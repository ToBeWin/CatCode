# CatCode Phase 1 Design Spec

> 日期: 2026-05-11
> 状态: Approved
> 实现方案: 方案 C — 自底向上 + 垂直验证

---

## 1. 项目概述

CatCode 是一个模型无关的开源 AI 编程 Agent，Rust 实现，TUI 优先，守护进程架构。

Phase 1 目标：实现完整的最小可运行内核，包含 7 个 crate，跑通"用户输入 → LLM 调用 → 中间件处理 → tool 执行 → 结果返回"的完整循环。

### 1.1 设计原则

- **中间件链模式**替代扁平 harness（借鉴 Deer Flow）
- **三层记忆架构**：Working / Session / Archive（综合 Claude Code + Deer Flow）
- **DeepSeek 作为首个 provider**（国内可访问，API 兼容 OpenAI 格式）
- **ratatui 作为 TUI 框架**
- **全面测试**：单元测试 + 集成测试 + mock provider

### 1.2 参考项目借鉴

| 借鉴内容 | 来源 | 说明 |
|---|---|---|
| 中间件链模式 | Deer Flow | 替代扁平 harness，可组合、可测试 |
| Loop Detection | Deer Flow | 滑动窗口 + hash + warn/stop 阈值 |
| Tool Error Handling | Deer Flow | 异常转 ToolMessage，不崩溃 |
| Guardrails | Deer Flow | 操作分级检查，fail-closed |
| Sandbox Middleware | Deer Flow | lazy init，跨 turn 复用 |
| Summarization | Deer Flow | context 自动压缩 |
| Memory 类型系统 | Claude Code | 4 类型（user/feedback/project/reference）+ frontmatter |
| Memory 文件存储 | Claude Code | Markdown + frontmatter，人可读 |
| MEMORY.md 索引 | Claude Code | 200 行上限，始终注入 context |
| Hook 系统 | Claude Code | 外部脚本扩展点 |
| Tool Permission | Claude Code | 工具权限检查 |
| Async Memory Update | Deer Flow | LLM 摘要 + debounce 队列 |

---

## 2. Workspace 结构

```
catcode/
├── Cargo.toml                    # workspace root
├── CLAUDE.md                     # 架构规范文档
├── AGENTS.md                     # Agent 运行规则
├── .catcode/                     # 运行时数据目录
│   ├── catcode.db                # SQLite
│   ├── config.toml               # 用户配置
│   ├── memory/                   # Session Memory 文件
│   │   ├── MEMORY.md             # 索引文件
│   │   └── *.md                  # 记忆文件
│   └── logs/
│
├── crates/
│   ├── catcode-core/             # 核心类型 + trait，零 IO
│   ├── catcode-provider/         # Provider 实现
│   ├── catcode-middleware/        # 中间件链（原 catcode-harness）
│   ├── catcode-tools/            # 内置工具
│   ├── catcode-context/          # Context 工程 + 记忆系统
│   ├── catcode-daemon/           # 守护进程 binary
│   └── catcode-tui/              # TUI binary
│
└── skills/                       # 内置 Skill 库
    ├── git.toml
    ├── rust.toml
    └── python.toml
```

**注意**：原 CLAUDE.md 中的 `catcode-harness` 改名为 `catcode-middleware`，反映中间件链架构。

---

## 3. catcode-core：核心类型与 Trait

零 IO 依赖，只定义类型和 trait。

### 3.1 Provider Trait

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn display_name(&self) -> &str;
    fn supported_models(&self) -> Vec<ModelInfo>;
    fn capabilities(&self) -> ProviderCapabilities;

    async fn chat(
        &self,
        request: ChatRequest,
        ctx: &ProviderContext,
    ) -> Result<ChatStream, ProviderError>;

    async fn health_check(&self) -> Result<(), ProviderError>;
    fn token_counter(&self) -> Box<dyn TokenCounter>;
}
```

### 3.2 核心数据类型

```rust
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub system: Option<String>,
    pub max_tokens: Option<u64>,
    pub temperature: Option<f32>,
    pub stream: bool,
}

pub struct ChatResponse {
    pub content: Vec<ContentBlock>,
    pub usage: TokenUsage,
    pub stop_reason: StopReason,
    pub model: String,
}

pub enum ContentBlock {
    Text { text: String },
    ToolCall { id: String, name: String, args: serde_json::Value },
    Thinking { text: String },
}

pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    StopSequence,
}
```

### 3.3 Tool Trait

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn operation_level(&self) -> OperationLevel;
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult;
}

pub enum OperationLevel {
    Safe,       // 直接执行
    Sensitive,  // 记录审计
    Dangerous,  // 需要审批
}

pub struct ToolResult {
    pub output: String,
    pub is_error: bool,
    pub metadata: serde_json::Value,
}
```

### 3.4 Middleware Trait

```rust
#[async_trait]
pub trait Middleware: Send + Sync {
    fn name(&self) -> &str;

    // Agent 生命周期
    async fn before_agent(&self, ctx: &mut AgentContext) -> Result<()> { Ok(()) }
    async fn after_agent(&self, ctx: &mut AgentContext) -> Result<()> { Ok(()) }

    // Model 调用钩子
    async fn before_model(&self, ctx: &mut AgentContext, request: &mut ChatRequest) -> Result<()> { Ok(()) }
    async fn after_model(&self, ctx: &mut AgentContext, response: &ChatResponse) -> Result<()> { Ok(()) }

    // Tool 调用包装（可拦截、修改、阻止）
    async fn wrap_tool_call(
        &self,
        ctx: &mut AgentContext,
        call: &ToolCall,
        next: ToolCallNext<'_>,
    ) -> ToolResult;
}

pub struct AgentContext {
    pub session_id: SessionId,
    pub messages: Vec<Message>,
    pub tool_outputs: VecDeque<ToolOutput>,
    pub token_usage: TokenUsage,
    pub metadata: HashMap<String, serde_json::Value>,
}
```

### 3.5 错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum CatCodeError {
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),

    #[error("Middleware error: {0}")]
    Middleware(#[from] MiddlewareError),

    #[error("Context error: {0}")]
    Context(#[from] ContextError),

    #[error("Config error: {0}")]
    Config(#[from] ConfigError),
}
```

---

## 4. catcode-provider：Provider 实现

### 4.1 实现顺序

1. **DeepSeek** — 第一个实现，API 兼容 OpenAI 格式
2. **Mock Provider** — 测试用，可配置返回值
3. **Anthropic** — 支持 prompt cache
4. **Ollama** — 本地部署

### 4.2 ProviderRegistry

```rust
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn register(&mut self, provider: Arc<dyn Provider>);
    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>>;
    pub fn list_healthy(&self) -> Vec<Arc<dyn Provider>>;
    pub fn load_from_config(&mut self, config: &Config) -> Result<()>;
}
```

### 4.3 ModelInfo

```rust
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub input_price_per_mtok: f64,
    pub output_price_per_mtok: f64,
    pub context_window: u64,
    pub tier: ModelTier,
}

pub enum ModelTier {
    Fast,
    Balanced,
    Powerful,
}
```

---

## 5. catcode-middleware：中间件链

替代原 CLAUDE.md 中的 `catcode-harness`。

### 5.1 中间件执行引擎

```rust
pub struct MiddlewareChain {
    middlewares: Vec<Box<dyn Middleware>>,
}

impl MiddlewareChain {
    pub async fn execute_agent(&self, ctx: &mut AgentContext, provider: &dyn Provider) -> Result<()> {
        // before_agent
        for mw in &self.middlewares {
            mw.before_agent(ctx).await?;
        }

        // agent loop
        loop {
            // before_model
            for mw in &self.middlewares {
                mw.before_model(ctx, &mut request).await?;
            }

            // model call
            let response = provider.chat(request, ctx).await?;

            // after_model
            for mw in &self.middlewares {
                mw.after_model(ctx, &response).await?;
            }

            // tool calls
            if let Some(tool_calls) = response.tool_calls() {
                for call in tool_calls {
                    let result = self.execute_tool(ctx, call, &tool_registry).await;
                    ctx.add_tool_result(call.id, result);
                }
            } else {
                break; // agent 完成
            }
        }

        // after_agent
        for mw in &self.middlewares {
            mw.after_agent(ctx).await?;
        }

        Ok(())
    }

    async fn execute_tool(&self, ctx: &mut AgentContext, call: &ToolCall, registry: &ToolRegistry) -> ToolResult {
        // 用闭包链实现 wrap_tool_call
        let handler = |call: &ToolCall| async {
            let tool = registry.get(&call.name).unwrap();
            tool.execute(call.args.clone(), ctx).await
        };

        let mut chain = handler;
        for mw in self.middlewares.iter().rev() {
            let prev = chain;
            chain = move |call| mw.wrap_tool_call(ctx, call, prev).await;
        }
        chain(call).await
    }
}
```

### 5.2 内置中间件列表

| 中间件 | 功能 | 借鉴来源 |
|---|---|---|
| `LoopDetectionMiddleware` | 检测重复 tool call，warn 后 hard stop | Deer Flow |
| `ToolErrorHandlingMiddleware` | tool 异常转错误消息 | Deer Flow |
| `SandboxMiddleware` | 沙盒隔离，lazy init | Deer Flow |
| `GuardrailMiddleware` | 操作分级检查 | Deer Flow |
| `RetryMiddleware` | 重试策略（exponential backoff） | CLAUDE.md |
| `TimeoutMiddleware` | 请求超时控制 | CLAUDE.md |
| `TokenUsageMiddleware` | token 用量记录 | Deer Flow |
| `SummarizationMiddleware` | context 自动压缩 | Deer Flow |
| `MemoryMiddleware` | 异步记忆更新，debounce | Deer Flow + Claude Code |
| `ToolPermissionMiddleware` | 工具权限检查 | Claude Code |

### 5.3 Loop Detection（详细设计）

```rust
pub struct LoopDetectionMiddleware {
    warn_threshold: u32,      // 默认 3
    hard_limit: u32,          // 默认 5
    window_size: usize,       // 默认 20
    history: HashMap<SessionId, VecDeque<String>>,
    warned: HashMap<SessionId, HashSet<String>>,
}
```

检测策略：
1. 每次 model 返回 tool calls 后，hash(tool_name + args)
2. 在滑动窗口中计数
3. 达到 warn_threshold → 注入警告消息
4. 达到 hard_limit → 强制移除 tool_calls，输出最终结果

### 5.4 Guardrails（详细设计）

```rust
pub enum GuardrailDecision {
    Allow,
    Deny { reason: String },
}

pub trait GuardrailProvider: Send + Sync {
    fn evaluate(&self, tool: &str, args: &serde_json::Value, level: OperationLevel) -> GuardrailDecision;
}
```

- `Safe` 操作：直接放行
- `Sensitive` 操作：记录审计日志
- `Dangerous` 操作：默认需要人工审批，可配置为 auto-approve

### 5.5 ModelRouter

```rust
pub struct ModelRouter {
    strategy: RoutingStrategy,
}

pub enum RoutingStrategy {
    Fixed(String),
    CostAware {
        simple_model: String,
        powerful_model: String,
        complexity_threshold: f32,
    },
    Fallback(Vec<String>),
}
```

---

## 6. catcode-tools：内置工具

### 6.1 工具列表

| 工具名 | 级别 | 说明 |
|---|---|---|
| `read_file` | Safe | 读取文件，支持行范围 |
| `write_file` | Sensitive | 写入/创建文件 |
| `patch_file` | Sensitive | 精确补丁 |
| `list_dir` | Safe | 目录列表 |
| `search_files` | Safe | ripgrep 封装 |
| `glob` | Safe | 文件名模式匹配 |
| `bash` | Dangerous | Shell 命令执行 |
| `git_status` | Safe | git 状态 |
| `git_diff` | Safe | git diff |
| `git_commit` | Sensitive | git 提交 |

### 6.2 ToolRegistry

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    deferred: DeferredToolRegistry,  // 当工具数 > 20 时启用
}

impl ToolRegistry {
    pub fn register(&mut self, tool: Arc<dyn Tool>);
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>>;
    pub fn list(&self) -> Vec<ToolMeta>;
    pub fn to_llm_schema(&self) -> Vec<serde_json::Value>;
    pub fn search(&self, query: &str) -> Vec<&dyn Tool>;
}
```

### 6.3 Tool Search（延迟发现）

当工具数超过阈值（20 个）时：
1. 只暴露核心工具给 LLM
2. 注册 `tool_search` 工具
3. Agent 通过 `tool_search(query="git")` 动态发现工具

---

## 7. catcode-context：Context 工程 + 记忆系统

### 7.1 三层记忆架构

```
┌─────────────────────────────────────────────────────────┐
│  Layer 1: Working Memory（工作记忆）                      │
│  生命周期：单次对话                                        │
│  存储：内存（ContextStack.working）                        │
│  内容：当前文件、最近工具输出、当前错误                    │
└─────────────────────────────────────────────────────────┘
                           │ 对话结束后摘要
                           ▼
┌─────────────────────────────────────────────────────────┐
│  Layer 2: Session Memory（会话记忆）                      │
│  生命周期：跨对话持久                                      │
│  存储：文件系统（.catcode/memory/ 目录）                   │
│  格式：Markdown + frontmatter                            │
│  索引：MEMORY.md（200 行上限，始终注入 context）           │
│  类型：user / feedback / project / reference              │
│  更新：Agent 自主写入 + 异步 LLM 摘要                     │
└─────────────────────────────────────────────────────────┘
                           │ 超过容量时压缩
                           ▼
┌─────────────────────────────────────────────────────────┐
│  Layer 3: Archive Memory（归档记忆）                      │
│  生命周期：长期保留                                        │
│  存储：SQLite（memory_facts 表）                          │
│  格式：结构化 JSON（带 confidence + category）             │
│  容量：默认 100 条 facts                                  │
│  更新：LLM 异步提取，debounce 30s                         │
└─────────────────────────────────────────────────────────┘
```

### 7.2 记忆类型（借鉴 Claude Code）

```rust
pub enum MemoryType {
    User,      // 用户画像
    Feedback,  // 行为偏好
    Project,   // 项目上下文
    Reference, // 外部引用
}
```

每个记忆文件格式：
```markdown
---
name: 记忆名称
description: 一行描述（用于相关性判断）
type: user|feedback|project|reference
---

记忆内容。对于 feedback/project 类型，结构为：规则/事实，然后 **Why:** 和 **How to apply:** 行。
```

### 7.3 记忆更新机制（借鉴 Deer Flow）

```rust
pub struct MemoryUpdater {
    debounce_duration: Duration,  // 默认 30s
    queue: VecDeque<ConversationContext>,
    provider: Arc<dyn Provider>,  // 用于 LLM 摘要
}

pub struct ArchiveFact {
    pub id: String,
    pub content: String,
    pub category: FactCategory,    // preference/knowledge/context/behavior/goal
    pub confidence: f32,           // 0.0 - 1.0
    pub created_at: DateTime<Utc>,
    pub source: String,
}
```

### 7.4 分层 Context 模型

```rust
pub struct ContextStack {
    permanent: PermanentLayer,   // 每次必带
    session: SessionLayer,       // 压缩后的会话摘要
    working: WorkingLayer,       // 当前工作集
    cold: ColdStorage,           // 历史归档
}

pub struct PermanentLayer {
    system_prompt: String,       // < 500 tokens
    project_rules: String,       // AGENTS.md 精华
    user_preferences: String,    // 从 Session Memory 注入
    model_profile_hint: String,
}
```

### 7.5 压缩流水线

```rust
pub struct ContextCompressor;

impl ContextCompressor {
    pub async fn compress(&self, stack: &mut ContextStack, target_tokens: u64) -> Result<()> {
        self.dedup_tool_outputs(stack);
        self.compress_tool_outputs(stack);
        self.compress_file_contents(stack);
        self.roll_session_history(stack).await;
        self.filter_by_relevance(stack);
    }
}
```

### 7.6 Token 预算

```rust
pub struct TokenBudget {
    pub session_limit: u64,
    pub per_request_limit: u64,
    pub warning_threshold: f32,     // 默认 0.80
    pub on_limit_reached: BudgetPolicy,
    pub input_used: u64,
    pub output_used: u64,
    pub cache_hits: u64,
    pub estimated_cost_usd: f64,
}

pub enum BudgetPolicy {
    Pause,
    AutoCompress,
    FallbackModel,
    HardStop,
}
```

---

## 8. catcode-daemon：守护进程

### 8.1 SessionManager

```rust
pub struct SessionManager {
    sessions: HashMap<SessionId, Arc<RwLock<Session>>>,
    max_concurrent: usize,         // 默认 5
    middleware_chain: Arc<MiddlewareChain>,
}

impl SessionManager {
    pub async fn create_session(&self, config: SessionConfig) -> SessionId;
    pub async fn pause_session(&self, id: SessionId) -> Result<()>;
    pub async fn resume_session(&self, id: SessionId) -> Result<()>;
    pub async fn cancel_session(&self, id: SessionId) -> Result<()>;
    pub async fn list_sessions(&self) -> Vec<SessionSummary>;
}
```

### 8.2 Agent Loop

```rust
pub struct AgentLoop {
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    middleware: Arc<MiddlewareChain>,
    context: ContextStack,
    budget: TokenBudget,
}

impl AgentLoop {
    pub async fn run(&mut self, user_input: &str) -> Result<AgentResponse> {
        // 1. 构建 context
        // 2. 执行中间件 before_agent
        // 3. 循环：model call → after_model → tool execution
        // 4. 执行中间件 after_agent
        // 5. 更新记忆
    }
}
```

### 8.3 Checkpoint

```rust
pub struct CheckpointManager;

impl CheckpointManager {
    pub async fn save(&self, session: &Session) -> Result<CheckpointId>;
    pub async fn restore(&self, session_id: SessionId) -> Result<Session>;
    pub async fn list(&self, session_id: SessionId) -> Vec<CheckpointMeta>;
}
```

---

## 9. catcode-tui：终端界面

### 9.1 布局

```
┌────────────────────────────────────────────────────────────────┐
│ CatCode  [session: fix-auth]  [deepseek-chat]  [💰$0.023]      │  顶栏
├──────────────┬─────────────────────────────────┬───────────────┤
│              │                                 │               │
│  Sessions    │      主内容区                    │  Token/Cost   │
│  面板        │                                 │  面板         │
│              │  Agent 思考 / 工具调用 / 输出   │               │
│  ● fix-auth  │  流式展示                       │  Input: 45.2K │
│  ○ refactor  │                                 │  Cache: 38.1K │
│              │                                 │  Output: 8.4K │
│  [+] 新建    │                                 │  Cost: $0.023 │
│              │                                 │               │
├──────────────┴─────────────────────────────────┴───────────────┤
│  > 输入框（支持多行，/ 触发命令）                               │  底栏
└────────────────────────────────────────────────────────────────┘
```

### 9.2 快捷键

```
Ctrl+N          新建 session
Ctrl+W          关闭当前 session
Ctrl+Tab        切换 session
Ctrl+P          命令面板
Ctrl+M          切换模型
Ctrl+Z          暂停 Agent
Ctrl+R          恢复 Agent
Ctrl+C          中断当前操作
Ctrl+D          退出 TUI（daemon 继续）
```

### 9.3 命令面板

```
/model <name>          切换模型
/provider <name>       切换 Provider
/compact               压缩 context
/usage                 显示 token 用量
/checkpoint save       保存检查点
/checkpoint restore    恢复检查点
/skill load <name>     加载 Skill
/mcp connect <server>  连接 MCP
/sandbox on/off        切换沙盒
/help                  帮助
```

---

## 10. 持久化（SQLite Schema）

```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    state TEXT NOT NULL,
    project_dir TEXT NOT NULL,
    model_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    checkpoint_data BLOB
);

CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    token_count INTEGER,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id)
);

CREATE TABLE token_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER DEFAULT 0,
    cost_usd REAL NOT NULL,
    recorded_at INTEGER NOT NULL
);

CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    tool TEXT,
    args TEXT,
    level TEXT NOT NULL,
    approved_by TEXT,
    result TEXT,
    created_at INTEGER NOT NULL
);

CREATE TABLE memory_facts (
    id TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    category TEXT NOT NULL,
    confidence REAL NOT NULL,
    source TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
```

---

## 11. 配置文件（config.toml）

```toml
[daemon]
host = "127.0.0.1"
port = 7070
auto_start = true
max_concurrent_sessions = 5
checkpoint_interval_turns = 10

[defaults]
provider = "deepseek"
model = "deepseek-chat"
sandbox = true

[budget]
session_limit_tokens = 500000
per_request_limit_tokens = 50000
warning_threshold = 0.80
on_limit_reached = "pause"

[routing]
strategy = "fixed"

[context]
compression_enabled = true
compression_threshold_ratio = 0.75
dedup_tool_outputs = true
max_file_content_tokens = 8000

[sandbox]
default_backend = "auto"
allowed_paths = ["$PROJECT_DIR"]
network_policy = "deny"
approval_timeout_secs = 300

[providers.deepseek]
api_key = "$DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com"

[providers.ollama]
base_url = "http://localhost:11434"

[middleware]
enabled = [
    "loop_detection",
    "tool_error_handling",
    "retry",
    "timeout",
    "token_usage",
    "summarization",
    "memory",
    "guardrail",
]

[middleware.loop_detection]
warn_threshold = 3
hard_limit = 5
window_size = 20

[middleware.retry]
max_attempts = 3
base_delay_ms = 1000
max_delay_ms = 30000

[middleware.timeout]
request_timeout_secs = 120

[middleware.memory]
debounce_seconds = 30
max_facts = 100
fact_confidence_threshold = 0.7
max_injection_tokens = 2000

[middleware.summarization]
trigger_token_ratio = 0.75
keep_recent_turns = 5
```

---

## 12. 实现路线图（方案 C）

```
Step 1: catcode-core
  - 类型定义（ChatRequest, ChatResponse, ContentBlock, TokenUsage）
  - Trait 定义（Provider, Tool, Middleware）
  - 错误类型（thiserror）
  - 零外部 IO 依赖

Step 2: catcode-provider
  - DeepSeek 实现（OpenAI 兼容 API）
  - Mock Provider（可配置返回值）
  - ProviderRegistry
  - 单元测试 + 集成测试

Step 3: catcode-middleware + catcode-tools
  - MiddlewareChain 执行引擎
  - LoopDetectionMiddleware
  - ToolErrorHandlingMiddleware
  - RetryMiddleware + TimeoutMiddleware
  - ToolRegistry + 6 个内置工具（read_file, write_file, bash, search, glob, list_dir）
  - 用 mock provider 跑通 "provider → middleware → tools" 核心 loop

Step 4: catcode-context
  - ContextStack 分层模型
  - Session Memory（文件系统 + MEMORY.md 索引）
  - TokenBudget
  - 基础压缩（dedup + truncation）
  - Archive Memory（SQLite facts）

Step 5: catcode-daemon
  - SessionManager（单 session）
  - AgentLoop（完整循环）
  - CheckpointManager
  - CLI 交互（临时替代 TUI）

Step 6: catcode-tui
  - ratatui 基础布局
  - Session 面板 + 主内容区 + Token 面板
  - 输入处理 + 命令面板
  - 快捷键系统

Step 7: 集成测试 + 端到端验证
  - 完整 loop 测试
  - 中间件链测试
  - 记忆系统测试
  - TUI 手动验证
```

---

## 13. 禁止事项

- 禁止硬编码 API Key
- 禁止绕过 OperationLevel 检查
- 禁止在 catcode-core 中引入 IO 依赖
- 禁止删除 audit_log 记录
- 禁止未经 TokenBudget 检查发起模型调用
