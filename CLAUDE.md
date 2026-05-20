# CatCode — CLAUDE.md

> 这是 CatCode 的架构规范和路线图文档，包含已实现能力与目标设计。
> 当前实现事实以代码、AGENTS.md 和 README.md 为准；本文档中更超前的章节应视为设计目标。
> 遇到设计冲突时，优先遵循 AGENTS.md、当前代码边界和已通过测试的行为。

---

## 项目概述

CatCode 是一个**模型无关的开源 AI 编程 Agent**，以 Rust 实现，TUI 优先，守护进程架构。

**核心价值主张：**
- 任何主流模型（国内外）均可接入，体验一致
- Harness 工程层弥补模型能力差异，而非依赖模型本身
- 极致的 context 工程和 token 效率优化
- 后台多 SubAgent 并发运行，支持远程控制
- NativeSandbox 安全控制：路径检查、超时、输出截断、操作分级和审批门禁；容器级隔离属于路线图

**技术栈：**
- 语言：Rust（全栈，含 TUI 和 daemon）
- TUI：ratatui
- 异步运行时：tokio
- 持久化：SQLite（via sqlx）
- API 层：axum
- 序列化：serde + serde_json
- 日志：tracing + tracing-subscriber

---

## Cargo Workspace 结构

```
catcode/
├── Cargo.toml                 # workspace root
├── CLAUDE.md                  # 本文档
├── AGENTS.md                  # Agent 运行规则（跨 session 持久）
├── .catcode/                  # 运行时数据目录
│   ├── catcode.db             # SQLite 主数据库
│   ├── config.toml            # 用户配置
│   ├── checkpoints/           # Agent 状态快照
│   ├── logs/                  # 结构化日志
│   └── plugins/               # 本地插件目录
│
├── crates/
│   ├── catcode-core/          # 核心库 crate（无 IO，纯逻辑）
│   ├── catcode-daemon/        # 守护进程 binary
│   ├── catcode-tui/           # TUI binary
│   ├── catcode-cli/           # 非交互式 CLI binary
│   ├── catcode-provider/      # 模型 Provider 抽象 + 实现
│   ├── catcode-middleware/    # Middleware 可靠性层
│   ├── catcode-context/       # Context 工程 + Token 管理
│   ├── catcode-sandbox/       # 沙盒隔离层
│   ├── catcode-tools/         # 内置工具集
│   ├── catcode-plugin/        # Plugin/Skill/MCP 扩展系统
│   └── catcode-api/           # HTTP/WebSocket API（远程控制）
│
└── skills/                    # 内置 Skill 库
    ├── git.toml
    ├── rust.toml
    └── python.toml
```

---

## 一、系统分层架构

```
┌──────────────────────────────────────────────────────────────┐
│                      Client Layer                             │
│   catcode-tui  │  catcode-cli  │  Web UI  │  手机 App        │
└─────────────────────────────┬────────────────────────────────┘
                              │ WebSocket + REST + SSE
┌─────────────────────────────▼────────────────────────────────┐
│                   catcode-api (axum)                          │
│           认证 · 限流 · 路由 · 审计日志                       │
└─────────────────────────────┬────────────────────────────────┘
                              │
┌─────────────────────────────▼────────────────────────────────┐
│                  catcode-daemon                               │
│                                                              │
│  SessionManager  ─────────────────────────────────────────  │
│  │  创建/暂停/恢复/终止 Agent                                │
│  │  并发控制 · 优先级队列 · 资源限制                         │
│                                                              │
│  AgentLoopEngine  ─────────────────────────────────────────  │
│  │  每个 Agent 独立 tokio task                               │
│  │  状态机驱动 · checkpoint 定期保存                         │
│                                                              │
│  catcode-harness  ─────────────────────────────────────────  │
│  │  retry · timeout · circuit breaker                       │
│  │  output validation · fallback routing                    │
│  │  model profile · cost-aware routing                      │
│                                                              │
│  catcode-context  ─────────────────────────────────────────  │
│  │  分层 context · 智能压缩 · token 预算                    │
│  │  prompt cache 优化 · 相关性过滤                          │
│                                                              │
│  catcode-provider  ────────────────────────────────────────  │
│  │  统一 ProviderTrait · 多模型适配                          │
│                                                              │
│  catcode-tools  ───────────────────────────────────────────  │
│  │  内置工具 · 权限控制 · 操作分级                           │
│                                                              │
│  catcode-sandbox  ─────────────────────────────────────────  │
│  │  Docker · firejail · 文件系统白名单 · 网络控制            │
│                                                              │
│  catcode-plugin  ──────────────────────────────────────────  │
│  │  Skill · Plugin · MCP · WASM（未来）                     │
│                                                              │
│  Persistence (SQLite)  ────────────────────────────────────  │
│  │  session · history · task · audit_log · token_usage      │
└──────────────────────────────────────────────────────────────┘
```

**严格规则：上层只能调用下层，禁止跨层调用。catcode-core 不依赖任何 IO。**

---

## 二、catcode-provider：模型 Provider 层

### 2.1 核心 Trait

```rust
// crates/catcode-provider/src/lib.rs

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

pub struct ProviderCapabilities {
    pub supports_tool_call: bool,
    pub supports_vision: bool,
    pub supports_prompt_cache: bool,
    pub max_context_tokens: u64,
    pub supports_streaming: bool,
}

pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub input_price_per_mtok: f64,   // USD per million tokens
    pub output_price_per_mtok: f64,
    pub context_window: u64,
    pub tier: ModelTier,             // Fast / Balanced / Powerful
}

pub enum ModelTier {
    Fast,      // 便宜快速，简单任务
    Balanced,  // 均衡
    Powerful,  // 最强能力，复杂任务
}
```

### 2.2 内置 Provider 列表

按优先级实现顺序：

| Provider | 模型示例 | 备注 |
|---|---|---|
| Anthropic | claude-sonnet-4, claude-opus-4 | 支持 prompt cache |
| OpenAI | gpt-4o, o3 | |
| DeepSeek | deepseek-chat, deepseek-reasoner | 重点支持，国内可访问 |
| Qwen (DashScope) | qwen3, qwen3-vl | 阿里云 |
| Google | gemini-2.5-pro | |
| Ollama | 任意本地模型 | 本地部署 |
| OpenRouter | 聚合多模型 | |
| OpenAI-Compatible | 任意兼容端点 | 用户自定义 baseURL |
| MiniMax | minimax-text | 国内 |
| Volcengine | 豆包系列 | 国内 |

### 2.3 Provider Registry

```rust
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn register(&mut self, provider: Arc<dyn Provider>);
    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>>;
    pub fn list_healthy(&self) -> Vec<Arc<dyn Provider>>;

    // 从配置文件动态加载用户自定义 provider
    pub fn load_from_config(&mut self, config: &Config) -> Result<()>;
}
```

---

## 三、catcode-harness：可靠性层

### 3.1 Harness 核心

```rust
pub struct AgentHarness {
    retry_policy: RetryPolicy,
    timeout_policy: TimeoutPolicy,
    circuit_breaker: CircuitBreaker,
    output_validator: OutputValidator,
    model_router: ModelRouter,
}

impl AgentHarness {
    pub async fn execute(
        &self,
        request: ChatRequest,
        provider: Arc<dyn Provider>,
    ) -> Result<ChatResponse, HarnessError>;
}
```

### 3.2 Retry 策略

```rust
pub struct RetryPolicy {
    pub max_attempts: u32,          // 默认 3
    pub backoff: BackoffStrategy,   // Exponential
    pub base_delay_ms: u64,         // 默认 1000
    pub max_delay_ms: u64,          // 默认 30000
    pub retryable_errors: Vec<ErrorKind>,
}

// 可重试的错误类型
pub enum RetryableError {
    RateLimit,           // 429，退避后重试
    Timeout,             // 请求超时
    ServerError,         // 5xx
    MalformedOutput,     // 输出格式错误，重试并附加格式纠错 prompt
    ToolCallParseError,  // tool call 解析失败
}
```

### 3.3 Circuit Breaker

```rust
pub struct CircuitBreaker {
    state: CircuitState,
    failure_threshold: u32,      // 连续失败 N 次后熔断
    recovery_timeout_secs: u64,  // 熔断后等待时间
    half_open_max_calls: u32,    // 半开状态允许的探测次数
}

pub enum CircuitState {
    Closed,    // 正常
    Open,      // 熔断，直接 fallback
    HalfOpen,  // 探测恢复中
}
```

### 3.4 Output Validator

```rust
pub struct OutputValidator;

impl OutputValidator {
    // 验证 tool call 格式，失败则返回纠错 prompt
    pub fn validate_tool_calls(&self, output: &str) -> ValidationResult;
    // 验证输出是否包含有害操作
    pub fn validate_safety(&self, output: &str) -> SafetyResult;
}
```

### 3.5 Model Router（cost-aware）

```rust
pub struct ModelRouter {
    strategy: RoutingStrategy,
}

pub enum RoutingStrategy {
    Fixed(String),           // 固定使用某模型
    CostAware {              // 根据任务复杂度路由
        simple_model: String,
        powerful_model: String,
        complexity_threshold: f32,
    },
    Fallback(Vec<String>),   // 按优先级 fallback
}

impl ModelRouter {
    pub fn select_model(
        &self,
        task: &Task,
        budget: &TokenBudget,
        provider_health: &ProviderHealth,
    ) -> String;
}
```

### 3.6 Model Profile（弱模型适配）

针对不同能力的模型，自动调整 prompt 策略：

```rust
pub struct ModelProfile {
    pub model_id: String,
    pub instruction_style: InstructionStyle,
    pub tool_call_format: ToolCallFormat,
    pub max_tools_per_turn: u32,
    pub prefers_simple_prompts: bool,
}

pub enum InstructionStyle {
    Concise,    // 强模型：简洁 prompt
    Explicit,   // 弱模型：详细分步指令
    ChainOfThought, // 推理模型：鼓励思考过程
}
```

---

## 四、catcode-context：Context 工程层

### 4.1 分层 Context 模型

```rust
pub struct ContextStack {
    permanent: PermanentLayer,   // 每次必带，极度精简
    session: SessionLayer,       // 压缩后的会话摘要
    working: WorkingLayer,       // 当前工作集，动态加载卸载
    cold: ColdStorage,           // 历史归档，按需检索
}

pub struct PermanentLayer {
    system_prompt: String,       // 压缩到 < 500 tokens
    project_rules: String,       // AGENTS.md 精华
    user_preferences: String,
    model_profile_hint: String,
}

pub struct SessionLayer {
    task_description: String,
    completed_steps_summary: String,  // 已完成步骤的摘要
    key_decisions: Vec<Decision>,     // 关键决策记录
    error_history: Vec<ErrorRecord>,  // 错误历史（去重）
}

pub struct WorkingLayer {
    current_files: Vec<FileContext>,   // 当前打开的文件
    recent_tool_outputs: VecDeque<ToolOutput>,  // 最近 N 次工具输出
    relevant_symbols: Vec<Symbol>,     // 相关代码符号
    current_errors: Vec<Error>,        // 当前错误
}
```

### 4.2 压缩流水线

```rust
pub struct ContextCompressor;

impl ContextCompressor {
    // 完整压缩流水线
    pub async fn compress(
        &self,
        stack: &mut ContextStack,
        target_tokens: u64,
    ) -> Result<()> {
        self.dedup_tool_outputs(stack);
        self.compress_tool_outputs(stack);
        self.compress_file_contents(stack);
        self.roll_session_history(stack).await;
        self.filter_by_relevance(stack);
    }

    // 语义去重：相同文件多次读取只保留最新
    fn dedup_tool_outputs(&self, stack: &mut ContextStack);

    // 工具输出截断：bash 输出超过阈值保留头尾+摘要
    fn compress_tool_outputs(&self, stack: &mut ContextStack);

    // 文件内容：只保留被修改的行 ± 上下文窗口
    fn compress_file_contents(&self, stack: &mut ContextStack);

    // 对话历史滚动：超过阈值用小模型生成摘要替换
    async fn roll_session_history(&self, stack: &mut ContextStack);

    // 相关性过滤：当前任务无关的文件不带入
    fn filter_by_relevance(&self, stack: &mut ContextStack);
}
```

### 4.3 Token 预算系统

```rust
pub struct TokenBudget {
    // 配置
    pub session_limit: u64,
    pub per_request_limit: u64,
    pub warning_threshold: f32,      // 默认 0.80
    pub on_limit_reached: BudgetPolicy,

    // 实时追踪
    pub input_used: u64,
    pub output_used: u64,
    pub cache_hits: u64,

    // 成本
    pub estimated_cost_usd: f64,
    pub saved_by_cache_usd: f64,
}

pub enum BudgetPolicy {
    Pause,           // 暂停，通知用户
    AutoCompress,    // 自动压缩 context
    FallbackModel,   // 切换便宜模型
    HardStop,
}

impl TokenBudget {
    pub fn record_usage(&mut self, usage: &TokenUsage);
    pub fn remaining_ratio(&self) -> f32;
    pub fn should_warn(&self) -> bool;
    pub fn is_exhausted(&self) -> bool;
    pub fn cost_summary(&self) -> CostSummary;
}
```

### 4.4 Prompt Cache 优化

```rust
pub struct PromptCacheOptimizer;

impl PromptCacheOptimizer {
    // 在构建请求时，标记稳定内容为 cache boundary
    // 永久层 + 会话层稳定部分 → cache_control: ephemeral
    // 工作层 + 新消息 → 不缓存（每次变化）
    pub fn apply_cache_hints(&self, request: &mut ChatRequest);

    // 估算 cache 命中率
    pub fn estimate_cache_savings(&self, request: &ChatRequest) -> f64;
}
```

---

## 五、catcode-daemon：守护进程

### 5.1 启动方式

```
# 首次运行自动检测并启动 daemon
catcode

# 显式控制
catcode daemon start    # 启动
catcode daemon stop     # 停止
catcode daemon status   # 状态
catcode daemon restart  # 重启
```

**行为规则：**
- `catcode` 任意子命令执行前，自动检测 daemon 是否运行
- daemon 未运行则自动在后台启动，然后继续执行命令
- daemon 默认监听 `127.0.0.1:7070`（本地）
- 远程访问需要显式配置 + 认证

### 5.2 Session Manager

```rust
pub struct SessionManager {
    sessions: HashMap<SessionId, Arc<RwLock<Session>>>,
    max_concurrent: usize,         // 默认 5，可配置
    task_queue: PriorityQueue<Task>,
    resource_monitor: ResourceMonitor,
}

impl SessionManager {
    pub async fn create_session(&self, config: SessionConfig) -> SessionId;
    pub async fn pause_session(&self, id: SessionId) -> Result<()>;
    pub async fn resume_session(&self, id: SessionId) -> Result<()>;
    pub async fn cancel_session(&self, id: SessionId) -> Result<()>;
    pub async fn list_sessions(&self) -> Vec<SessionSummary>;
    pub async fn get_session(&self, id: SessionId) -> Option<SessionSnapshot>;
}

pub struct Session {
    pub id: SessionId,
    pub state: SessionState,
    pub agent: AgentLoop,
    pub budget: TokenBudget,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub project_dir: PathBuf,
}

pub enum SessionState {
    Running,
    Paused,
    WaitingForHumanReview(ReviewRequest),
    WaitingForApproval(ApprovalRequest),
    Completed,
    Failed(Error),
}
```

### 5.3 Agent Loop 状态机

```
         ┌──────┐
         │ Idle │
         └──┬───┘
            │ 收到任务
            ▼
       ┌─────────┐
       │ Running │ ◄──────────────────────┐
       └────┬────┘                        │
            │                             │
     ┌──────┼──────┐                      │
     │      │      │                      │
     ▼      ▼      ▼                      │
  Tool   LLM    Human                     │
  Call   Call   Review                    │
     │      │      │                      │
     │      │   用户回复                  │
     │      │      │                      │
     └──────┴──────┴──────────────────────┘
            │ 任务完成
            ▼
       ┌──────────┐
       │ Complete │
       └──────────┘
```

### 5.4 Checkpoint 系统

```rust
pub struct CheckpointManager;

impl CheckpointManager {
    // 每 N 轮对话自动保存
    pub async fn save(&self, session: &Session) -> Result<CheckpointId>;
    // daemon 重启后恢复
    pub async fn restore(&self, session_id: SessionId) -> Result<Session>;
    // 列出所有检查点
    pub async fn list(&self, session_id: SessionId) -> Vec<CheckpointMeta>;
    // 回滚到某个检查点
    pub async fn rollback(&self, checkpoint_id: CheckpointId) -> Result<()>;
}
```

---

## 六、catcode-sandbox：沙盒层

### 6.1 操作安全分级

```rust
pub enum OperationLevel {
    // 🟢 安全：直接执行，记录日志
    Safe,
    // 🟡 敏感：记录审计，可配置自动/手动
    Sensitive,
    // 🔴 危险：默认进沙盒 + 人工审批
    Dangerous,
}

// 工具操作分级映射
impl OperationLevel {
    pub fn classify(tool: &str, args: &ToolArgs) -> Self {
        match tool {
            "read_file" | "search" | "glob" | "list_dir" => Self::Safe,
            "write_file" | "patch_file" | "git_commit" => Self::Sensitive,
            "bash" | "delete_file" | "network_request" => Self::Dangerous,
            _ => Self::Dangerous, // 未知操作默认危险
        }
    }
}
```

### 6.2 沙盒后端

```rust
#[async_trait]
pub trait SandboxBackend: Send + Sync {
    async fn execute(&self, cmd: &Command, policy: &SandboxPolicy) -> Result<Output>;
    fn is_available(&self) -> bool;
}

// 按优先级选择可用后端
pub struct SandboxSelector {
    backends: Vec<Box<dyn SandboxBackend>>,
}
// 实现：DockerSandbox > FirejailSandbox > BubblewrapSandbox > NativeSandbox

pub struct SandboxPolicy {
    pub allowed_paths: Vec<PathBuf>,   // 白名单路径
    pub denied_paths: Vec<PathBuf>,    // 黑名单路径
    pub network_access: NetworkPolicy, // 禁止/允许/白名单
    pub memory_limit_mb: u64,
    pub cpu_limit_percent: f32,
    pub timeout_secs: u64,
}

pub enum NetworkPolicy {
    Deny,
    Allow,
    Whitelist(Vec<String>),
}
```

### 6.3 人工审批门控

```rust
pub struct ApprovalGate;

impl ApprovalGate {
    // 发送审批请求，阻塞等待用户响应
    pub async fn request_approval(
        &self,
        operation: &Operation,
        timeout_secs: u64,
    ) -> ApprovalResult;
}

pub enum ApprovalResult {
    Approved,
    ApprovedAlways,    // 本次会话内同类操作自动批准
    Rejected,
    Timeout,           // 超时默认拒绝
}
```

---

## 七、catcode-tools：工具层

### 7.1 内置工具

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn operation_level(&self) -> OperationLevel;
    async fn execute(&self, args: ToolArgs, ctx: &ToolContext) -> ToolResult;
}
```

**内置工具列表：**

| 工具名 | 级别 | 说明 |
|---|---|---|
| `read_file` | 🟢 Safe | 读取文件，支持行范围 |
| `write_file` | 🟡 Sensitive | 写入/创建文件 |
| `patch_file` | 🟡 Sensitive | 精确补丁，最小化写入 |
| `list_dir` | 🟢 Safe | 目录列表 |
| `search_files` | 🟢 Safe | 全文搜索（ripgrep） |
| `glob` | 🟢 Safe | 文件名模式匹配 |
| `bash` | 🔴 Dangerous | Shell 命令执行 |
| `git_status` | 🟢 Safe | git 状态 |
| `git_diff` | 🟢 Safe | git diff |
| `git_commit` | 🟡 Sensitive | git 提交 |
| `delete_file` | 🔴 Dangerous | 删除文件 |
| `web_fetch` | 🔴 Dangerous | HTTP 请求 |
| `code_analysis` | 🟢 Safe | AST 分析（tree-sitter） |

### 7.2 Tool Registry

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn register(&mut self, tool: Arc<dyn Tool>);
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>>;
    pub fn list(&self) -> Vec<ToolMeta>;
    pub fn to_llm_schema(&self) -> Vec<serde_json::Value>;

    // 插件工具注册入口
    pub fn register_plugin_tools(&mut self, plugin: &dyn Plugin);
    // MCP 工具注册入口
    pub fn register_mcp_tools(&mut self, server: &McpServer);
}
```

---

## 八、catcode-plugin：扩展系统

### 8.1 三层扩展体系

```
┌─────────────────────────────────────────┐
│  Skill（技能）                           │
│  TOML 配置文件，无代码                   │
│  定义：prompt模板·工具组合·工作流        │
│  示例：skills/rust.toml                  │
└─────────────────────────────────────────┘
┌─────────────────────────────────────────┐
│  Plugin（插件）                          │
│  动态加载的 Rust/WASM 库                 │
│  可注册新工具·新 Provider·新 Harness策略 │
└─────────────────────────────────────────┘
┌─────────────────────────────────────────┐
│  MCP（Model Context Protocol）          │
│  标准协议，连接外部 MCP Server           │
│  支持任意第三方 MCP 服务                 │
└─────────────────────────────────────────┘
```

### 8.2 Skill 格式

```toml
# skills/rust.toml
[skill]
name = "rust"
version = "1.0.0"
description = "Rust 项目开发专用技能"

[rules]
always_run = ["cargo check", "cargo clippy"]
prefer_tools = ["code_analysis", "patch_file"]
avoid_tools = []

[prompts]
system_suffix = """
你正在处理 Rust 项目。遵循以下规则：
- 使用 anyhow::Result 处理错误
- 优先使用 patch_file 而非 write_file
- 修改后必须运行 cargo check 验证
"""

[context]
always_include_files = ["Cargo.toml", "src/lib.rs"]
ignore_patterns = ["target/", "*.lock"]

[hooks]
before_commit = "cargo test"
after_write = "cargo fmt"
```

### 8.3 Plugin Trait

```rust
pub trait Plugin: Send + Sync {
    fn metadata(&self) -> PluginMetadata;
    fn on_load(&self, registry: &mut PluginRegistry) -> Result<()>;
    fn on_unload(&self) -> Result<()>;

    // 可选：注册额外工具
    fn tools(&self) -> Vec<Arc<dyn Tool>> { vec![] }
    // 可选：注册额外 Provider
    fn providers(&self) -> Vec<Arc<dyn Provider>> { vec![] }
    // 可选：注册 Harness 策略
    fn harness_patches(&self) -> Vec<HarnessPatch> { vec![] }
}
```

### 8.4 MCP 集成

```rust
pub struct McpClient {
    server_config: McpServerConfig,
    connection: McpConnection,
}

impl McpClient {
    pub async fn connect(config: McpServerConfig) -> Result<Self>;
    pub async fn list_tools(&self) -> Result<Vec<McpTool>>;
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value>;
    pub async fn list_resources(&self) -> Result<Vec<McpResource>>;
}

// 配置示例（config.toml）
// [mcp.servers.github]
// command = "npx"
// args = ["-y", "@modelcontextprotocol/server-github"]
// env = { GITHUB_TOKEN = "$GITHUB_TOKEN" }
```

---

## 九、catcode-api：远程控制层

### 9.1 API 设计

```
# Session 管理
GET    /api/v1/sessions              # 列出所有 session
POST   /api/v1/sessions              # 创建新 session
GET    /api/v1/sessions/:id          # 获取 session 状态
DELETE /api/v1/sessions/:id          # 终止 session
GET    /api/v1/sessions/:id/audit    # 查看审计日志
GET    /api/v1/sessions/:id/messages # 查看消息历史
GET    /api/v1/sessions/:id/recovery # 查看恢复计划
GET    /api/v1/sessions/:id/usage    # 查看 token 用量
POST   /api/v1/sessions/:id/pause    # 暂停
POST   /api/v1/sessions/:id/resume   # 恢复
POST   /api/v1/sessions/:id/message  # 发送消息给 Agent

# 实时流
GET    /api/v1/sessions/:id/stream   # SSE 实时日志流
WS     /api/v1/sessions/:id/ws       # WebSocket 双向通信

# 审批
GET    /api/v1/approvals             # 待审批列表
POST   /api/v1/approvals/:id         # 审批/拒绝操作

# 模型管理
GET    /api/v1/providers             # Provider 列表
GET    /api/v1/providers/health      # 健康状态
GET    /api/v1/models                # 可用模型列表

# Token & 成本
GET    /api/v1/usage                 # 用量统计
GET    /api/v1/usage/sessions/:id    # 单 session 用量

# 系统
GET    /api/v1/health                # daemon 健康检查
GET    /api/v1/version               # 版本信息
```

### 9.2 认证

```rust
pub struct AuthConfig {
    pub mode: AuthMode,
    pub token: Option<String>,       // Bearer token
    pub allowed_origins: Vec<String>,
}

pub enum AuthMode {
    LocalOnly,      // 只监听 127.0.0.1，无需认证
    TokenAuth,      // Bearer token 认证
    MutualTLS,      // mTLS（未来企业版）
}
```

### 9.3 SSE 事件格式

```json
// Agent 执行事件
{"type": "agent_thinking", "session_id": "xxx", "content": "..."}
{"type": "tool_call", "session_id": "xxx", "tool": "read_file", "args": {...}}
{"type": "tool_result", "session_id": "xxx", "result": "..."}
{"type": "agent_message", "session_id": "xxx", "content": "..."}
{"type": "approval_required", "session_id": "xxx", "operation": {...}}
{"type": "session_state", "session_id": "xxx", "state": "paused"}
{"type": "token_usage", "session_id": "xxx", "input": 1234, "output": 456, "cost_usd": 0.012}
{"type": "budget_warning", "session_id": "xxx", "used_ratio": 0.82}
{"type": "error", "session_id": "xxx", "error": "..."}
```

---

## 十、TUI 设计（catcode-tui）

### 10.1 布局

```
┌────────────────────────────────────────────────────────────────┐
│ CatCode  [session: fix-auth]  [claude-sonnet-4]  [💰$0.023]   │  顶栏
├──────────────┬─────────────────────────────────┬───────────────┤
│              │                                 │               │
│  Sessions    │      主内容区                    │  Token/Cost   │
│  面板        │                                 │  面板         │
│              │  Agent 思考 / 工具调用 / 输出   │               │
│  ● fix-auth  │  流式展示                       │  Input: 45.2K │
│  ○ refactor  │                                 │  Cache: 38.1K │
│  ○ tests     │                                 │  Output: 8.4K │
│              │                                 │  Cost: $0.023 │
│  [+] 新建    │                                 │  节省: 74%    │
│              │                                 │               │
├──────────────┴─────────────────────────────────┴───────────────┤
│  > 输入框（支持多行，/ 触发命令）                               │  底栏
└────────────────────────────────────────────────────────────────┘
```

### 10.2 快捷键系统（命令切换极度方便）

```
# 全局快捷键
Ctrl+N          新建 session
Ctrl+W          关闭当前 session
Ctrl+Tab        切换到下一个 session
Ctrl+Shift+Tab  切换到上一个 session
Ctrl+1~9        直接跳到第 N 个 session
Ctrl+P          命令面板（模糊搜索所有命令）
Ctrl+M          切换模型（快速弹窗选择）
Ctrl+B          切换 Provider
Ctrl+,          打开配置
Ctrl+K          清空当前对话
Ctrl+Z          暂停当前 Agent
Ctrl+R          恢复暂停的 Agent
Ctrl+C          中断当前操作（单次）
Ctrl+D          退出 TUI（daemon 继续后台运行）
Ctrl+Q          退出 TUI + 停止 daemon

# Session 面板
J/K 或 ↑/↓     选择 session
Enter           进入 session
D               删除选中 session
P               暂停/恢复选中 session

# 主内容区
PageUp/PageDown 滚动历史
Ctrl+F          搜索历史内容
Y               复制最后一条 Agent 输出
Ctrl+Y          复制选中内容

# 审批弹窗
Y               批准操作
N               拒绝操作
A               本次会话内同类操作自动批准
```

### 10.3 命令面板（/ 触发）

```
/model <name>          切换模型
/provider <name>       切换 Provider
/budget <tokens>       设置预算
/budget-policy <mode>  设置预算策略
/skill load <name>     加载 Skill
/skill list            列出已加载 Skill
/mcp connect <server>  连接 MCP Server
/mcp list              列出已连接 MCP
/sandbox on/off        切换沙盒模式
/compact               立即压缩 context
/checkpoint save       手动保存检查点
/checkpoint list       列出检查点
/checkpoint restore    恢复检查点
/usage                 显示 token 用量
/export                导出对话记录
/help                  帮助
```

---

## 十一、持久化层（SQLite Schema）

```sql
-- 会话表
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    state TEXT NOT NULL,           -- Running/Paused/Completed/Failed
    project_dir TEXT NOT NULL,
    model_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    checkpoint_data BLOB           -- 序列化的 Agent 状态
);

-- 消息历史表
CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,            -- user/assistant/tool
    content TEXT NOT NULL,
    token_count INTEGER,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(id)
);

-- Token 用量表
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

-- 审计日志（不可删除）
CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    operation TEXT NOT NULL,       -- 操作类型
    tool TEXT,
    args TEXT,                     -- JSON
    level TEXT NOT NULL,           -- Safe/Sensitive/Dangerous
    approved_by TEXT,              -- human/auto/policy
    result TEXT,                   -- success/rejected/error
    created_at INTEGER NOT NULL
);

-- 插件配置表
CREATE TABLE plugins (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    enabled INTEGER DEFAULT 1,
    config TEXT                    -- JSON
);

-- MCP Server 配置表
CREATE TABLE mcp_servers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    command TEXT NOT NULL,
    args TEXT,                     -- JSON array
    env TEXT,                      -- JSON object
    enabled INTEGER DEFAULT 1
);
```

---

## 十二、配置文件（config.toml）

```toml
[daemon]
host = "127.0.0.1"
port = 7070
auto_start = true              # 首次运行自动启动
max_concurrent_sessions = 5
checkpoint_interval_turns = 10

[auth]
mode = "local_only"            # local_only | token | mtls
# token = "your-secret-token" # 远程访问时启用

[defaults]
provider = "anthropic"
model = "claude-sonnet-4-5"
sandbox = true

[budget]
session_limit_tokens = 500000
per_request_limit_tokens = 50000
warning_threshold = 0.80
on_limit_reached = "pause"     # pause | auto_compress | fallback | stop

[routing]
strategy = "cost_aware"        # fixed | cost_aware | fallback
simple_model = "deepseek-chat"
powerful_model = "claude-sonnet-4-5"
complexity_threshold = 0.6

[context]
compression_enabled = true
compression_threshold_ratio = 0.75  # context 用到 75% 时压缩
dedup_tool_outputs = true
max_file_content_tokens = 8000

[sandbox]
default_backend = "auto"       # auto | docker | firejail | native
allowed_paths = ["$PROJECT_DIR"]
network_policy = "deny"
memory_limit_mb = 512
cpu_limit_percent = 50.0
approval_timeout_secs = 300

[observability]
log_level = "info"
log_format = "json"
token_tracking = true
cost_tracking = true

# Provider 配置
[providers.anthropic]
api_key = "$ANTHROPIC_API_KEY"
base_url = "https://api.anthropic.com"

[providers.deepseek]
api_key = "$DEEPSEEK_API_KEY"
base_url = "https://api.deepseek.com"

[providers.ollama]
base_url = "http://localhost:11434"

# 自定义 OpenAI 兼容 endpoint
[providers.custom]
api_key = "$CUSTOM_API_KEY"
base_url = "https://your-endpoint.com/v1"
models = ["your-model-name"]

# MCP Server 配置
[mcp.servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_TOKEN = "$GITHUB_TOKEN" }

# Skill 配置
[skills]
auto_detect = true             # 根据项目自动加载对应 Skill
enabled = ["rust", "git"]
```

---

## 十三、实现规则（Claude Code 必读）

### 13.1 Rust 编码规范

```
- 错误处理：所有公开 API 使用 anyhow::Result，内部使用 thiserror 定义错误类型
- 异步：全面使用 tokio，禁止阻塞调用出现在 async 上下文
- 并发：优先使用 Arc<RwLock<T>>，避免 Mutex 死锁风险
- 日志：使用 tracing 宏（trace!/debug!/info!/warn!/error!），禁止 println!
- 测试：每个 crate 的 src/lib.rs 底部包含 #[cfg(test)] 模块
- 格式：cargo fmt 后提交，clippy 零警告
- 文档：pub 函数必须有 doc comment
```

### 13.2 架构规则

```
- catcode-core 禁止依赖任何 IO crate（tokio fs、reqwest 等）
- Provider 的具体实现禁止泄漏到 Harness 层以上
- Tool 执行必须经过 OperationLevel 分级检查
- 所有写操作必须记录 audit_log
- Agent 状态变更必须通过 SSE 广播到所有连接的客户端
- Token 用量必须在每次 API 调用后立即记录
```

### 13.3 开发优先级（严格按顺序）

```
Phase 1 - 可运行的最小内核：
  [x] catcode-provider: Anthropic + DeepSeek + Ollama
  [x] catcode-harness: retry + timeout + output validation
  [x] catcode-tools: read_file + write_file + bash + search
  [x] catcode-context: 分层模型 + 基础压缩 + token 追踪
  [x] catcode-daemon: 单 session + checkpoint
  [x] catcode-tui: 基础布局 + 快捷键 + 命令面板

Phase 2 - 多 Agent + 沙盒：
  [ ] SessionManager: 多 session 并发
  [ ] catcode-sandbox: firejail + 操作分级
  [ ] SubAgent 支持

Phase 3 - 远程控制：
  [ ] catcode-api: REST + SSE + WebSocket
  [ ] 认证系统
  [ ] Web UI 基础版

Phase 4 - 扩展生态：
  [ ] catcode-plugin: Skill + Plugin 系统
  [ ] MCP 完整支持
  [ ] 更多 Provider

Phase 5 - 平台化：
  [ ] WASM 插件沙盒
  [ ] 手机 App
  [ ] 多用户 + 团队协作
```

### 13.4 禁止事项

```
- 禁止在任何 crate 中硬编码 API Key
- 禁止绕过 OperationLevel 检查直接执行危险操作
- 禁止在 catcode-core 中引入任何外部网络依赖
- 禁止删除 audit_log 中的任何记录
- 禁止在未经 TokenBudget 检查的情况下发起模型调用
```

---

## 十四、进化路线

```
v0.1  最小可用  ── 单 Agent · 3个Provider · 基础TUI · token追踪
v0.2  多 Agent ── SubAgent并发 · Session管理 · 后台Daemon
v0.3  沙盒     ── Docker隔离 · 操作分级 · 人工审批
v0.4  远程控制 ── WebSocket API · Web UI · 手机App基础
v0.5  扩展生态 ── Skill · Plugin · MCP完整支持
v0.6  智能路由 ── cost-aware · 自适应压缩 · prompt cache优化
v1.0  平台化   ── 多用户 · 团队协作 · 企业部署 · WASM插件
```

---

*本文档随项目演进持续更新。重大架构变更需更新本文档后再实现。*
