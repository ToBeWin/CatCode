# CatCode 增强功能设计文档

> **日期:** 2026-05-12
> **状态:** 已批准
> **实现顺序:** Plan/Act Mode → Goal 模式 → 评测系统 → 猫咪宠物

---

## 一、Plan/Act Mode（计划/执行模式）

### 1.1 目标

提供三种工作模式，让用户在 AI 编程过程中有更精细的控制：

- **Plan Mode**: Agent 只分析和规划，不执行任何修改操作，输出执行计划供用户审批
- **Act Mode**: Agent 按照计划或直接响应执行工具调用（默认模式）
- **Auto Mode**: 先输出计划，用户确认后自动切换到 act 执行

### 1.2 设计

**模式切换命令：**
```
/plan                       # 进入计划模式
/act                        # 进入执行模式（默认）
/auto                       # 自动模式：先 plan 再 act
```

**实现机制：**

1. **App 层**：`AppMode` 枚举跟踪当前模式
2. **UI 层**：状态栏显示当前模式标识 `[Plan]` `[Act]` `[Auto]`
3. **Session 层**：模式状态持久化到 session
4. **Agent Loop 层**：根据模式决定 system prompt 注入和工具可用性

**Plan Mode 行为：**
- system prompt 注入："你处于计划模式。只能分析和规划，禁止执行任何工具调用。输出详细的执行计划。"
- 工具调用被拦截，返回提示信息
- 用户发送 `/act` 或 `/auto` 切换模式后继续执行

**Auto Mode 行为：**
- Agent 先输出计划（标记为 `## Plan`）
- TUI 显示确认提示：`[E]xecute  [M]odify  [C]ancel`
- 用户按 E 后自动切换到 Act Mode 执行

### 1.3 涉及文件

| 文件 | 修改内容 |
|------|----------|
| `catcode-tui/src/app.rs` | 添加 AppMode 枚举、/plan /act /auto 命令 |
| `catcode-tui/src/ui.rs` | 状态栏显示模式标识 |
| `catcode-tui/src/lib.rs` | 快捷键支持（Ctrl+P 切换 plan/act） |

---

## 二、Goal 模式（目标驱动自主编程）

### 2.1 目标

参考 Codex 的 `/goal` 系统，实现目标驱动的自主编程循环：

- 用户设定一个目标，Agent 自主执行直到完成
- 支持 token 预算限制
- 支持暂停/恢复

### 2.2 设计

**命令：**
```
/goal <objective>           # 创建目标，启动自主循环
/goal status                # 查看当前目标状态
/goal pause                 # 暂停目标
/goal resume                # 恢复目标
/goal budget <tokens>       # 设置 token 预算
/goal clear                 # 清除目标
```

**数据结构：**
```rust
pub struct Goal {
    pub objective: String,
    pub status: GoalStatus,
    pub token_budget: Option<u64>,
    pub tokens_used: u64,
    pub started_at: Instant,
    pub elapsed_seconds: u64,
}

pub enum GoalStatus {
    Active,
    Paused,
    BudgetLimited,
    Complete,
}
```

**自主循环机制：**
1. Agent 完成一轮响应后，检查是否有活跃 Goal
2. 如果有，自动注入继续执行的 prompt："目标尚未完成，请继续执行。"
3. 如果 token 预算耗尽，自动暂停并通知用户
4. 如果 Agent 判断目标已完成，自动标记为 Complete

### 2.3 涉及文件

| 文件 | 修改内容 |
|------|----------|
| `catcode-tui/src/app.rs` | Goal 数据结构、/goal 命令、自主循环逻辑 |
| `catcode-tui/src/ui.rs` | Goal 状态显示 |

---

## 三、评测系统

### 3.1 目标

内建评测能力，追踪各 Provider+Model 组合的表现。

### 3.2 设计

**命令：**
```
/benchmark run              # 运行标准测试集
/benchmark results          # 查看评测结果
/benchmark compare          # 对比不同模型表现
```

**追踪指标：**
- 任务成功率（pass@1）
- 平均 token 消耗
- 平均耗时
- 成本（USD）

**数据存储：** SQLite benchmark_results 表

### 3.3 涉及文件

| 文件 | 修改内容 |
|------|----------|
| `catcode-tui/src/app.rs` | /benchmark 命令 |
| `catcode-daemon/` | benchmark 执行引擎 |

---

## 四、猫咪宠物效果

### 4.1 目标

利用 CatCode 的 "Cat" 品牌，在 TUI 中显示 ASCII art 猫咪动画。

### 4.2 设计

**命令：**
```
/cat on                     # 开启猫咪显示
/cat off                    # 关闭猫咪显示
/cat style <name>           # 切换猫咪风格
```

**状态动画：**
- 空闲：`( =^._.^= )` 睡觉
- 思考中：`( =^.^= )` 眼睛转动
- 执行中：`( =^.^= )ﾉ` 敲键盘
- 出错：`( =O.O= )` 惊讶
- 完成：`( =^.^= )~` 开心

### 4.3 涉及文件

| 文件 | 修改内容 |
|------|----------|
| `catcode-tui/src/app.rs` | /cat 命令、猫咪状态机 |
| `catcode-tui/src/ui.rs` | 猫咪 ASCII art 渲染 |

---

## 实现进度

| 功能 | 状态 | 完成日期 |
|------|------|----------|
| Plan/Act Mode | 已完成 | 2026-05-12 |
| Goal 模式 | 已完成 | 2026-05-12 |
| 猫咪宠物 | 已完成 | 2026-05-12 |
| 评测系统 | 待开始 | - |
