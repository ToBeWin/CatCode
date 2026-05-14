use catcode_daemon::{
    AgentRuntime, AgentRuntimeOptions, BenchmarkCase, BenchmarkReport, Session, SessionManager,
    SessionState, default_benchmark_cases,
};
use std::path::PathBuf;
use std::time::Instant;
use tokio::sync::mpsc;

/// Events from the agent loop to the TUI.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Agent produced text output.
    /// [`AgentMessage`].
    AgentMessage(String),
    /// Real-time thinking content delta.
    /// [`Thinking`].
    Thinking(String),
    /// Agent is calling a tool.
    /// [`ToolCall`].
    ToolCall { tool: String, args: String },
    /// Tool execution completed.
    /// [`ToolResult`].
    ToolResult { tool: String, output: String },
    /// Status updates during processing (e.g. "Calling DeepSeek...").
    /// [`StatusUpdate`].
    StatusUpdate(String),
    /// Agent finished processing.
    /// [`Completed`].
    Completed,
    /// Agent encountered an error.
    /// [`Error`].
    Error(String),
    /// Token usage update.
    /// [`TokenUpdate`].
    TokenUpdate { input: u64, output: u64, cache: u64 },
}

/// Input mode for the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    /// Normal mode — typing messages to the agent.
    /// [`Normal`].
    Normal,
    /// Command mode — typing a `/` command.
    /// [`Command`].
    Command,
}

/// Cat mascot state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatState {
    /// Idle — cat is sleeping.
    /// [`Idle`].
    Idle,
    /// Thinking — cat is pondering.
    /// [`Thinking`].
    Thinking,
    /// Executing — cat is working.
    /// [`Executing`].
    Executing,
    /// Error — cat is surprised.
    /// [`Error`].
    Error,
    /// Done — cat is happy.
    /// [`Done`].
    Done,
}

impl CatState {
    pub fn ascii_art(&self) -> &'static str {
        match self {
            CatState::Idle => CAT_IDLE,
            CatState::Thinking => CAT_THINKING,
            CatState::Executing => CAT_EXECUTING,
            CatState::Error => CAT_ERROR,
            CatState::Done => CAT_DONE,
        }
    }

    /// Label.
    pub fn label(&self) -> &'static str {
        match self {
            CatState::Idle => "sleeping",
            CatState::Thinking => "thinking",
            CatState::Executing => "working",
            CatState::Error => "surprised",
            CatState::Done => "happy",
        }
    }
}

const CAT_IDLE: &str = "\
  |`.    .'|
  |  \\_/  |
  | (^‥^) |
  |> zZ   |";
const CAT_THINKING: &str = "\
  |`.    .'|
  |  \\_/  |
  | (^..^) |
  |> ...  |";
const CAT_EXECUTING: &str = "\
  |`.    .'|
  |  \\_/  |
  | (｀^´) |
  |> ==>  |";
const CAT_ERROR: &str = "\
  |`.    .'|
  |  \\_/  |
  | (；ェ；)|
  |> !! ! |";
const CAT_DONE: &str = "\
  |`.    .'|
  |  \\_/  |
  | (^‥^) |
  |>  ✓  |";

/// Agent execution mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentMode {
    /// Plan mode — agent only analyzes and plans, no tool execution.
    /// [`Plan`].
    Plan,
    /// Act mode — agent executes tools normally (default).
    /// [`Act`].
    Act,
    /// Auto mode — agent plans first, then executes after user approval.
    /// [`Auto`].
    Auto,
}

/// Goal status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalStatus {
    /// [`Active`].
    Active,
    /// [`Paused`].
    Paused,
    /// [`BudgetLimited`].
    BudgetLimited,
    /// [`Complete`].
    Complete,
}

/// A goal that drives autonomous agent execution.
#[derive(Debug, Clone)]
pub struct Goal {
    pub objective: String,
    pub status: GoalStatus,
    pub token_budget: Option<u64>,
    pub tokens_used: u64,
    pub started_at: Instant,
}

impl AgentMode {
    pub fn label(&self) -> &'static str {
        match self {
            AgentMode::Plan => "Plan",
            AgentMode::Act => "Act",
            AgentMode::Auto => "Auto",
        }
    }

    /// Description.
    pub fn description(&self) -> &'static str {
        match self {
            AgentMode::Plan => "Planning only — no tool execution",
            AgentMode::Act => "Normal execution — tools available",
            AgentMode::Auto => "Plan first, execute after approval",
        }
    }
}

/// A chat message displayed in the main content area.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    pub thinking: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// [`MessageRole`]
pub enum MessageRole {
    /// [`User`].
    User,
    /// [`Assistant`].
    Assistant,
    /// [`System`].
    System,
    /// [`Tool`].
    Tool,
}

/// Application state for the TUI.
pub struct App {
    /// Session manager for creating/managing sessions.
    pub sessions: SessionManager,
    /// Currently active session ID.
    pub active_session: Option<String>,
    /// Chat messages for the active session.
    pub messages: Vec<ChatMessage>,
    /// Current input text.
    pub input: String,
    /// Cursor position in the input.
    pub input_cursor: usize,
    /// Current input mode.
    pub input_mode: InputMode,
    /// Command palette input (when in Command mode).
    pub command_input: String,
    /// Whether the app should quit.
    pub should_quit: bool,
    /// Project directory.
    pub project_dir: PathBuf,
    /// Scroll offset for the messages view.
    pub scroll_offset: usize,
    /// Token usage display.
    pub token_display: TokenDisplay,
    /// Status message for the bottom bar.
    pub status: String,
    /// Current agent execution mode.
    pub agent_mode: AgentMode,
    /// Active goal (if any).
    pub goal: Option<Goal>,
    /// Whether the cat mascot is enabled.
    pub cat_enabled: bool,
    /// Current cat mascot state.
    pub cat_state: CatState,
    /// Benchmark test cases.
    pub benchmark_cases: Vec<BenchmarkCase>,
    /// Benchmark reports (results).
    pub benchmark_reports: Vec<BenchmarkReport>,
    /// Channel for receiving agent events.
    pub agent_event_rx: Option<mpsc::UnboundedReceiver<AgentEvent>>,
    /// Channel for sending agent events (cloned for background tasks).
    pub agent_event_tx: Option<mpsc::UnboundedSender<AgentEvent>>,
    /// Whether the agent is currently processing.
    pub agent_busy: bool,
    /// Status message to show while busy (e.g. "Calling DeepSeek...")
    pub busy_message: String,
    /// Spinner animation frame counter (0..=3, cycles through |/-\)
    pub spinner_frame: u8,
    /// Input history for up/down navigation.
    pub input_history: Vec<String>,
    /// Current position in input history (None = not navigating).
    pub history_index: Option<usize>,
    /// Saved input when navigating history.
    pub history_saved_input: String,
    /// Real-time thinking content from the current agent response.
    pub current_thinking: String,
}

#[derive(Debug, Clone, Default)]
/// [`TokenDisplay`]
pub struct TokenDisplay {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_tokens: u64,
    pub cost_usd: f64,
}

impl App {
    pub fn new(project_dir: PathBuf) -> Self {
        Self {
            sessions: SessionManager::new(5),
            active_session: None,
            messages: Vec::new(),
            input: String::new(),
            input_cursor: 0,
            input_mode: InputMode::Normal,
            command_input: String::new(),
            should_quit: false,
            project_dir,
            scroll_offset: 0,
            token_display: TokenDisplay::default(),
            status: String::new(),
            agent_mode: AgentMode::Act,
            goal: None,
            cat_enabled: true,
            cat_state: CatState::Idle,
            benchmark_cases: default_benchmark_cases(),
            benchmark_reports: Vec::new(),
            agent_event_rx: None,
            agent_event_tx: None,
            agent_busy: false,
            busy_message: String::new(),
            spinner_frame: 0,
            input_history: Vec::new(),
            history_index: None,
            history_saved_input: String::new(),
            current_thinking: String::new(),
        }
    }

    /// Create a new session and make it active.
    pub fn create_session(&mut self, name: &str) {
        match self.sessions.create_session(
            name,
            self.project_dir.clone(),
            "deepseek-chat",
            "deepseek",
        ) {
            Ok(id) => {
                self.active_session = Some(id.clone());
                self.messages.clear();
                self.status = format!("Session '{}' created", name);
            }
            Err(e) => {
                self.status = format!("Error: {}", e);
            }
        }
    }

    /// Get the active session.
    pub fn active_session(&self) -> Option<&Session> {
        self.active_session
            .as_ref()
            .and_then(|id| self.sessions.get(id))
    }

    /// Add a chat message to the display.
    pub fn add_message(&mut self, role: MessageRole, content: impl Into<String>) {
        self.messages.push(ChatMessage {
            role,
            content: content.into(),
            thinking: None,
        });
        // Auto-scroll to bottom
        self.scroll_to_bottom();
    }

    /// Scroll to the bottom of the messages.
    pub fn scroll_to_bottom(&mut self) {
        // Will be adjusted by the UI based on viewport height
        self.scroll_offset = usize::MAX;
    }

    /// Scroll up by one page.
    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// Scroll down by one page.
    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
    }

    /// Handle a character input.
    pub fn handle_char(&mut self, c: char) {
        match self.input_mode {
            InputMode::Normal => {
                self.input.insert(self.input_cursor, c);
                self.input_cursor += c.len_utf8();
            }
            InputMode::Command => {
                self.command_input.push(c);
            }
        }
    }

    /// Navigate up in input history.
    pub fn history_up(&mut self) {
        if self.input_mode != InputMode::Normal || self.input_history.is_empty() {
            return;
        }
        match self.history_index {
            None => {
                // Save current input and go to latest
                self.history_saved_input = self.input.clone();
                self.history_index = Some(self.input_history.len() - 1);
                self.input = self.input_history[self.input_history.len() - 1].clone();
                self.input_cursor = self.input.len();
            }
            Some(i) if i > 0 => {
                self.history_index = Some(i - 1);
                self.input = self.input_history[i - 1].clone();
                self.input_cursor = self.input.len();
            }
            _ => {}
        }
    }

    /// Navigate down in input history.
    pub fn history_down(&mut self) {
        if self.input_mode != InputMode::Normal {
            return;
        }
        match self.history_index {
            Some(i) if i < self.input_history.len() - 1 => {
                self.history_index = Some(i + 1);
                self.input = self.input_history[i + 1].clone();
                self.input_cursor = self.input.len();
            }
            Some(_) => {
                // Back to current input
                self.history_index = None;
                self.input = self.history_saved_input.clone();
                self.input_cursor = self.input.len();
            }
            None => {}
        }
    }

    /// Handle backspace.
    pub fn handle_backspace(&mut self) {
        match self.input_mode {
            InputMode::Normal => {
                if self.input_cursor > 0 {
                    // Find the previous char boundary
                    let prev = self.input[..self.input_cursor]
                        .char_indices()
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.input_cursor = prev;
                    self.input.remove(prev);
                }
            }
            InputMode::Command => {
                self.command_input.pop();
            }
        }
    }

    /// Submit the current input.
    pub fn submit_input(&mut self) -> Option<String> {
        match self.input_mode {
            InputMode::Normal => {
                if self.input.is_empty() {
                    return None;
                }
                let text = self.input.clone();
                // Save to history (avoid consecutive duplicates)
                if self.input_history.last().map(|s| s.as_str()) != Some(&text) {
                    self.input_history.push(text.clone());
                }
                self.history_index = None;
                self.history_saved_input.clear();
                self.input.clear();
                self.input_cursor = 0;
                self.add_message(MessageRole::User, &text);
                Some(text)
            }
            InputMode::Command => {
                let cmd = self.command_input.trim().to_string();
                self.command_input.clear();
                self.input_mode = InputMode::Normal;
                if !cmd.is_empty() {
                    self.execute_command(&cmd);
                }
                None
            }
        }
    }

    /// Switch to command mode.
    pub fn enter_command_mode(&mut self) {
        self.input_mode = InputMode::Command;
        self.command_input.clear();
    }

    /// Switch back to normal mode.
    pub fn enter_normal_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.command_input.clear();
    }

    /// Execute a `/` command.
    fn execute_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.trim().splitn(2, ' ').collect();
        match parts[0] {
            "quit" | "q" => {
                self.should_quit = true;
            }
            "new" | "n" => {
                let name = if parts.len() > 1 {
                    parts[1]
                } else {
                    "new-session"
                };
                self.create_session(name);
            }
            "sessions" | "ls" => {
                let sessions = self.sessions.list();
                if sessions.is_empty() {
                    self.status = "No sessions".to_string();
                } else {
                    let names: Vec<String> = sessions
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            let state = match &s.state {
                                SessionState::Running => "●",
                                SessionState::Paused => "◐",
                                SessionState::Completed => "✓",
                                SessionState::Failed(_) => "✗",
                            };
                            format!("{}: {}{} ({})", i + 1, state, s.name, &s.id[..8])
                        })
                        .collect();
                    self.status = names.join(" | ");
                }
            }
            "switch" | "s" => {
                if let Some(num_str) = parts.get(1) {
                    if let Ok(num) = num_str.parse::<usize>() {
                        self.switch_to_session_by_index(num);
                    } else {
                        // Try to switch by name
                        self.switch_to_session_by_name(num_str);
                    }
                } else {
                    self.status = "Usage: /switch <number|name>".to_string();
                }
            }
            "close" => {
                if let Some(id) = self.active_session.clone() {
                    let _ = self.sessions.update_state(&id, SessionState::Completed);
                    self.active_session = None;
                    self.messages.clear();
                    self.status = "Session closed".to_string();
                } else {
                    self.status = "No active session".to_string();
                }
            }
            "clear" | "cls" => {
                self.messages.clear();
                self.scroll_offset = 0;
                self.status = "Messages cleared".to_string();
            }
            "usage" => {
                self.status = format!(
                    "Input: {} | Output: {} | Cache: {} | Cost: ${:.4}",
                    self.token_display.input_tokens,
                    self.token_display.output_tokens,
                    self.token_display.cache_tokens,
                    self.token_display.cost_usd,
                );
            }
            "model" | "m" => {
                if let Some(model) = parts.get(1) {
                    if let Some(session) = self.active_session_mut() {
                        session.model_id = model.to_string();
                        self.status = format!("Model set to: {}", model);
                    } else {
                        self.status = "No active session".to_string();
                    }
                } else {
                    let current = self
                        .active_session()
                        .map(|s| s.model_id.as_str())
                        .unwrap_or("none");
                    self.status = format!("Current model: {} | Usage: /model <name>", current);
                }
            }
            "plan" | "p" => {
                self.set_agent_mode(AgentMode::Plan);
            }
            "act" | "a" => {
                self.set_agent_mode(AgentMode::Act);
            }
            "auto" => {
                self.set_agent_mode(AgentMode::Auto);
            }
            "cat" | "c" => {
                if let Some(sub) = parts.get(1) {
                    match sub.trim() {
                        "on" => {
                            self.cat_enabled = true;
                            self.status = "Cat mascot enabled".to_string();
                        }
                        "off" => {
                            self.cat_enabled = false;
                            self.status = "Cat mascot disabled".to_string();
                        }
                        _ => {
                            self.status = "Usage: /cat on|off".to_string();
                        }
                    }
                } else {
                    self.cat_enabled = !self.cat_enabled;
                    self.status = if self.cat_enabled {
                        "Cat mascot enabled".to_string()
                    } else {
                        "Cat mascot disabled".to_string()
                    };
                }
            }
            "goal" | "g" => {
                if let Some(sub) = parts.get(1) {
                    let sub_parts: Vec<&str> = sub.splitn(2, ' ').collect();
                    match sub_parts[0] {
                        "status" => {
                            self.status = self.goal_status_display();
                        }
                        "pause" => {
                            self.pause_goal();
                        }
                        "resume" => {
                            self.resume_goal();
                        }
                        "budget" => {
                            if let Some(amount) = sub_parts.get(1) {
                                if let Ok(budget) = amount.parse::<u64>() {
                                    self.set_goal_budget(budget);
                                } else {
                                    self.status = "Usage: /goal budget <tokens>".to_string();
                                }
                            } else {
                                self.status = "Usage: /goal budget <tokens>".to_string();
                            }
                        }
                        "clear" => {
                            self.clear_goal();
                        }
                        "complete" => {
                            self.complete_goal();
                        }
                        _ => {
                            // /goal <objective> — create a new goal
                            self.create_goal(sub, None);
                        }
                    }
                } else {
                    self.status = self.goal_status_display();
                }
            }
            "benchmark" | "bench" | "b" => {
                if let Some(sub) = parts.get(1) {
                    match sub.trim() {
                        "list" | "ls" => {
                            let names: Vec<String> = self
                                .benchmark_cases
                                .iter()
                                .map(|c| format!("{}: {}", c.id, c.name))
                                .collect();
                            self.status = if names.is_empty() {
                                "No benchmark cases".to_string()
                            } else {
                                names.join(" | ")
                            };
                        }
                        "results" | "r" => {
                            if self.benchmark_reports.is_empty() {
                                self.status = "No benchmark results yet".to_string();
                            } else {
                                let summaries: Vec<String> = self
                                    .benchmark_reports
                                    .iter()
                                    .map(|r| r.summary_line())
                                    .collect();
                                self.status = summaries.join(" || ");
                            }
                        }
                        "clear" => {
                            self.benchmark_reports.clear();
                            self.status = "Benchmark results cleared".to_string();
                        }
                        _ => {
                            self.status = "Usage: /benchmark list|results|clear".to_string();
                        }
                    }
                } else {
                    self.status = format!(
                        "Benchmark: {} cases loaded | /benchmark list|results|clear",
                        self.benchmark_cases.len()
                    );
                }
            }
            "help" | "h" => {
                self.add_message(
                    MessageRole::System,
                    "=== CatCode Help ===\n\
                     \n\
                     Commands:\n\
                     /new <name>       Create a new session\n\
                     /sessions         List all sessions\n\
                     /switch <n|name>  Switch to session\n\
                     /close            Close current session\n\
                     /clear            Clear messages\n\
                     /model <name>     Set/view model\n\
                     /usage            Show token usage\n\
                     /plan             Enter plan mode (no tools)\n\
                     /act              Enter act mode (default)\n\
                     /auto             Plan first, then execute\n\
                     /goal <objective> Create autonomous goal\n\
                     /cat on|off       Toggle cat mascot\n\
                     /benchmark        Run evaluation tests\n\
                     /quit             Exit CatCode\n\
                     \n\
                     Keyboard Shortcuts:\n\
                     Enter             Send message\n\
                     Ctrl+P            Toggle plan/act mode\n\
                     Ctrl+N            New session\n\
                     Ctrl+W            Close session\n\
                     Ctrl+1-9          Switch to session N\n\
                     Ctrl+Up/Down      Input history\n\
                     Up/Down           Scroll messages\n\
                     PageUp/PageDown   Scroll faster\n\
                     Home/End          Jump to top/bottom",
                );
            }
            _ => {
                self.status = format!("Unknown command: /{}", parts[0]);
            }
        }
    }

    /// Switch to a session by its list index (1-based).
    pub fn switch_to_session_by_index(&mut self, index: usize) {
        let sessions = self.sessions.list();
        if index == 0 || index > sessions.len() {
            self.status = format!("Invalid index: {} (valid: 1-{})", index, sessions.len());
            return;
        }
        let session = &sessions[index - 1];
        self.active_session = Some(session.id.clone());
        self.messages.clear();
        self.scroll_offset = 0;
        self.status = format!("Switched to: {}", session.name);
    }

    /// Switch to a session by name substring match.
    fn switch_to_session_by_name(&mut self, name: &str) {
        let sessions = self.sessions.list();
        let name_lower = name.to_lowercase();
        if let Some(session) = sessions
            .iter()
            .find(|s| s.name.to_lowercase().contains(&name_lower))
        {
            self.active_session = Some(session.id.clone());
            self.messages.clear();
            self.scroll_offset = 0;
            self.status = format!("Switched to: {}", session.name);
        } else {
            self.status = format!("No session matching '{}'", name);
        }
    }

    /// Get the active session mutably.
    pub fn active_session_mut(&mut self) -> Option<&mut Session> {
        self.active_session
            .as_ref()
            .and_then(|id| self.sessions.get_mut(id))
    }

    /// Quit the application.
    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    /// Set the agent mode.
    pub fn set_agent_mode(&mut self, mode: AgentMode) {
        self.agent_mode = mode;
        self.status = format!(
            "Mode: {} — {}",
            self.agent_mode.label(),
            self.agent_mode.description()
        );
    }

    /// Toggle between Plan and Act mode.
    pub fn toggle_plan_act(&mut self) {
        match self.agent_mode {
            AgentMode::Plan => self.set_agent_mode(AgentMode::Act),
            AgentMode::Act | AgentMode::Auto => self.set_agent_mode(AgentMode::Plan),
        }
    }

    /// Get the system prompt suffix for the current agent mode.
    pub fn agent_mode_system_suffix(&self) -> Option<String> {
        match self.agent_mode {
            AgentMode::Plan => Some(
                "You are in PLAN MODE. You MUST NOT execute any tools. \
                 Only analyze the codebase and produce a detailed execution plan. \
                 Present your plan in a clear, numbered list of steps. \
                 Wait for the user to switch to Act mode before executing."
                    .to_string(),
            ),
            AgentMode::Act => None,
            AgentMode::Auto => Some(
                "You are in AUTO MODE. First, output a concise execution plan \
                 under a `## Plan` heading. Then wait for user confirmation \
                 before executing. If the user says 'go' or 'execute', \
                 proceed with tool calls."
                    .to_string(),
            ),
        }
    }

    // === Goal management ===

    /// Create a new goal.
    pub fn create_goal(&mut self, objective: &str, token_budget: Option<u64>) {
        self.goal = Some(Goal {
            objective: objective.to_string(),
            status: GoalStatus::Active,
            token_budget,
            tokens_used: 0,
            started_at: Instant::now(),
        });
        self.status = format!("Goal created: {}", objective);
    }

    /// Get the current goal status as a display string.
    pub fn goal_status_display(&self) -> String {
        match &self.goal {
            None => "No active goal".to_string(),
            Some(goal) => {
                let status = match goal.status {
                    GoalStatus::Active => "active",
                    GoalStatus::Paused => "paused",
                    GoalStatus::BudgetLimited => "limited by budget",
                    GoalStatus::Complete => "complete",
                };
                let elapsed = goal.started_at.elapsed().as_secs();
                let time_str = format_elapsed_time(elapsed);
                let mut parts = vec![
                    format!("Status: {}", status),
                    format!("Objective: {}", goal.objective),
                    format!("Time: {}", time_str),
                    format!("Tokens: {}", goal.tokens_used),
                ];
                if let Some(budget) = goal.token_budget {
                    parts.push(format!("Budget: {}", budget));
                }
                parts.join(" | ")
            }
        }
    }

    /// Pause the active goal.
    pub fn pause_goal(&mut self) {
        if let Some(goal) = &mut self.goal {
            if goal.status == GoalStatus::Active {
                goal.status = GoalStatus::Paused;
                self.status = "Goal paused".to_string();
            } else {
                self.status = format!("Cannot pause goal (status: {:?})", goal.status);
            }
        } else {
            self.status = "No active goal".to_string();
        }
    }

    /// Resume a paused goal.
    pub fn resume_goal(&mut self) {
        if let Some(goal) = &mut self.goal {
            if goal.status == GoalStatus::Paused {
                goal.status = GoalStatus::Active;
                self.status = "Goal resumed".to_string();
            } else {
                self.status = format!("Cannot resume goal (status: {:?})", goal.status);
            }
        } else {
            self.status = "No active goal".to_string();
        }
    }

    /// Set the token budget for the current goal.
    pub fn set_goal_budget(&mut self, budget: u64) {
        if let Some(goal) = &mut self.goal {
            goal.token_budget = Some(budget);
            self.status = format!("Goal budget set to {} tokens", budget);
        } else {
            self.status = "No active goal".to_string();
        }
    }

    /// Clear the current goal.
    pub fn clear_goal(&mut self) {
        self.goal = None;
        self.status = "Goal cleared".to_string();
    }

    /// Update goal token usage. Returns true if budget is exhausted.
    pub fn update_goal_tokens(&mut self, tokens: u64) -> bool {
        if let Some(goal) = &mut self.goal {
            goal.tokens_used += tokens;
            if let Some(budget) = goal.token_budget
                && goal.tokens_used >= budget
            {
                goal.status = GoalStatus::BudgetLimited;
                return true;
            }
        }
        false
    }

    /// Check if the goal is active and should drive autonomous execution.
    pub fn is_goal_active(&self) -> bool {
        self.goal
            .as_ref()
            .map(|g| g.status == GoalStatus::Active)
            .unwrap_or(false)
    }

    /// Mark the goal as complete.
    pub fn complete_goal(&mut self) {
        if let Some(goal) = &mut self.goal {
            goal.status = GoalStatus::Complete;
            self.status = format!("Goal completed: {}", goal.objective);
        }
    }

    // === Cat mascot ===

    /// Set the cat mascot state.
    pub fn set_cat_state(&mut self, state: CatState) {
        self.cat_state = state;
    }

    /// Get the cat ASCII art for the current state.
    pub fn cat_art(&self) -> &'static str {
        if self.cat_enabled {
            self.cat_state.ascii_art()
        } else {
            ""
        }
    }

    /// Check if there is active thinking content to display.
    pub fn has_thinking(&self) -> bool {
        !self.current_thinking.is_empty()
    }

    // === Benchmark ===

    /// Add a benchmark report.
    pub fn add_benchmark_report(&mut self, report: BenchmarkReport) {
        self.status = format!(
            "Benchmark: {}/{} — {}/{} passed ({:.0}%)",
            report.provider_id,
            report.model_id,
            report.passed,
            report.total_cases,
            report.pass_rate * 100.0
        );
        self.benchmark_reports.push(report);
    }

    /// Get the latest benchmark report formatted as a table.
    pub fn latest_benchmark_display(&self) -> String {
        match self.benchmark_reports.last() {
            Some(report) => catcode_daemon::format_report_table(report),
            None => "No benchmark results yet".to_string(),
        }
    }

    // === Agent communication ===

    /// Initialize the agent event channel.
    pub fn init_agent_channel(&mut self) {
        let (tx, rx) = mpsc::unbounded_channel();
        self.agent_event_tx = Some(tx);
        self.agent_event_rx = Some(rx);
    }

    /// Send a message to the agent and process it in the background.
    pub fn send_to_agent(&mut self, message: &str) {
        if self.agent_busy {
            self.status = "Agent is still processing...".to_string();
            return;
        }

        let tx = self.agent_event_tx.clone();
        if let Some(tx) = tx {
            self.agent_busy = true;
            self.set_cat_state(CatState::Thinking);
            let msg = message.to_string();
            let project_dir = self.project_dir.clone();

            tokio::spawn(async move {
                let _ = tx.send(AgentEvent::StatusUpdate("Analyzing request...".to_string()));

                match run_agent_once(&msg, project_dir).await {
                    Ok(result) => {
                        if let Some(plan) = result.auto_plan.as_deref()
                            && !plan.trim().is_empty()
                        {
                            let _ =
                                tx.send(AgentEvent::Thinking(format!("Plan:\n{}\n", plan.trim())));
                        }
                        let _ = tx.send(AgentEvent::StatusUpdate(
                            "Generating response...".to_string(),
                        ));
                        let _ = tx.send(AgentEvent::TokenUpdate {
                            input: result.total_usage.input_tokens,
                            output: result.total_usage.output_tokens,
                            cache: result.total_usage.cache_read_tokens,
                        });
                        let _ = tx.send(AgentEvent::AgentMessage(result.response));
                        let _ = tx.send(AgentEvent::Completed);
                    }
                    Err(err) => {
                        let _ = tx.send(AgentEvent::Error(err.to_string()));
                    }
                }
            });
        }
    }

    /// Advance the spinner animation frame. Called on each Tick.
    pub fn tick(&mut self) {
        if self.agent_busy {
            self.spinner_frame = (self.spinner_frame + 1) % 4;
        }
    }

    /// Process pending agent events. Returns true if any events were processed.
    pub fn poll_agent_events(&mut self) -> bool {
        // Collect events first to avoid borrow checker issues
        let mut events = Vec::new();
        if let Some(rx) = &mut self.agent_event_rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }

        let had_events = !events.is_empty();
        for event in events {
            match event {
                AgentEvent::AgentMessage(msg) => {
                    let thinking = if self.current_thinking.is_empty() {
                        None
                    } else {
                        Some(self.current_thinking.clone())
                    };
                    self.messages.push(ChatMessage {
                        role: MessageRole::Assistant,
                        content: msg,
                        thinking,
                    });
                    self.current_thinking.clear();
                    self.scroll_to_bottom();
                    self.set_cat_state(CatState::Done);
                }
                AgentEvent::Thinking(text) => {
                    self.current_thinking.push_str(&text);
                    self.set_cat_state(CatState::Thinking);
                }
                AgentEvent::ToolCall { tool, args } => {
                    self.add_message(
                        MessageRole::Tool,
                        format!("Calling tool: {}({})", tool, args),
                    );
                    self.set_cat_state(CatState::Executing);
                }
                AgentEvent::ToolResult { tool, output } => {
                    self.add_message(MessageRole::Tool, format!("{} result: {}", tool, output));
                }
                AgentEvent::StatusUpdate(msg) => {
                    self.busy_message = msg;
                    self.status = format!("⟳ {}", self.busy_message);
                }
                AgentEvent::Completed => {
                    self.agent_busy = false;
                    self.busy_message.clear();
                    self.spinner_frame = 0;
                    self.set_cat_state(CatState::Idle);
                }
                AgentEvent::Error(err) => {
                    self.add_message(MessageRole::System, format!("Error: {}", err));
                    self.agent_busy = false;
                    self.busy_message.clear();
                    self.spinner_frame = 0;
                    self.set_cat_state(CatState::Error);
                }
                AgentEvent::TokenUpdate {
                    input,
                    output,
                    cache,
                } => {
                    self.token_display.input_tokens += input;
                    self.token_display.output_tokens += output;
                    self.token_display.cache_tokens += cache;
                }
            }
        }
        had_events
    }
}

async fn run_agent_once(
    message: &str,
    project_dir: PathBuf,
) -> anyhow::Result<catcode_daemon::AgentLoopResult> {
    AgentRuntime::new()
        .run_once(
            message,
            &project_dir,
            AgentRuntimeOptions {
                system_prompt: "You are CatCode, a concise coding agent inside a terminal UI. Use tools when needed and keep responses focused.".to_string(),
                ..Default::default()
            },
        )
        .await
}

/// Format elapsed seconds into a human-readable string.
fn format_elapsed_time(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else {
        let hours = seconds / 3600;
        let mins = (seconds % 3600) / 60;
        format!("{}h {}m", hours, mins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> App {
        App::new(PathBuf::from("/tmp/test"))
    }

    #[test]
    fn test_app_new() {
        let app = make_app();
        assert!(app.active_session.is_none());
        assert!(app.messages.is_empty());
        assert!(app.input.is_empty());
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_handle_char() {
        let mut app = make_app();
        app.handle_char('h');
        app.handle_char('i');
        assert_eq!(app.input, "hi");
        assert_eq!(app.input_cursor, 2);
    }

    #[test]
    fn test_handle_char_unicode() {
        let mut app = make_app();
        app.handle_char('你');
        assert_eq!(app.input, "你");
        assert_eq!(app.input_cursor, 3); // '你' is 3 bytes in UTF-8

        app.handle_char('好');
        assert_eq!(app.input, "你好");
        assert_eq!(app.input_cursor, 6); // '好' is also 3 bytes
    }

    #[test]
    fn test_handle_backspace() {
        let mut app = make_app();
        app.handle_char('a');
        app.handle_char('b');
        app.handle_backspace();
        assert_eq!(app.input, "a");
        assert_eq!(app.input_cursor, 1);
    }

    #[test]
    fn test_handle_backspace_unicode() {
        let mut app = make_app();
        app.handle_char('你');
        app.handle_char('好');
        assert_eq!(app.input_cursor, 6);

        app.handle_backspace();
        assert_eq!(app.input, "你");
        assert_eq!(app.input_cursor, 3);

        app.handle_backspace();
        assert!(app.input.is_empty());
        assert_eq!(app.input_cursor, 0);
    }

    #[test]
    fn test_handle_backspace_at_start() {
        let mut app = make_app();
        app.handle_backspace();
        assert!(app.input.is_empty());
    }

    #[test]
    fn test_submit_input() {
        let mut app = make_app();
        app.handle_char('h');
        app.handle_char('i');
        let text = app.submit_input();
        assert_eq!(text, Some("hi".to_string()));
        assert!(app.input.is_empty());
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.messages[0].role, MessageRole::User);
    }

    #[test]
    fn test_submit_empty() {
        let mut app = make_app();
        let text = app.submit_input();
        assert!(text.is_none());
        assert!(app.messages.is_empty());
    }

    #[test]
    fn test_enter_command_mode() {
        let mut app = make_app();
        app.enter_command_mode();
        assert_eq!(app.input_mode, InputMode::Command);
    }

    #[test]
    fn test_command_quit() {
        let mut app = make_app();
        app.enter_command_mode();
        app.command_input = "quit".to_string();
        app.submit_input();
        assert!(app.should_quit);
    }

    #[test]
    fn test_empty_command_does_nothing() {
        let mut app = make_app();
        app.enter_command_mode();
        app.command_input = "".to_string();
        app.submit_input();
        assert!(!app.should_quit);
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_whitespace_only_command_does_nothing() {
        let mut app = make_app();
        app.enter_command_mode();
        app.command_input = "   ".to_string();
        app.submit_input();
        assert!(!app.should_quit);
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_command_new_session() {
        let mut app = make_app();
        app.enter_command_mode();
        app.command_input = "new test-session".to_string();
        app.submit_input();
        assert!(app.active_session.is_some());
        assert_eq!(app.sessions.total_count(), 1);
    }

    #[test]
    fn test_create_session() {
        let mut app = make_app();
        app.create_session("my-session");
        assert!(app.active_session.is_some());
        let session = app.active_session().unwrap();
        assert_eq!(session.name, "my-session");
    }

    #[test]
    fn test_add_message() {
        let mut app = make_app();
        app.add_message(MessageRole::User, "hello");
        app.add_message(MessageRole::Assistant, "hi there");
        assert_eq!(app.messages.len(), 2);
    }

    #[test]
    fn test_scroll() {
        let mut app = make_app();
        app.scroll_down(10);
        assert_eq!(app.scroll_offset, 10);
        app.scroll_up(5);
        assert_eq!(app.scroll_offset, 5);
        app.scroll_up(10);
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn test_command_switch_by_index() {
        let mut app = make_app();
        app.create_session("first");
        app.create_session("second");
        app.create_session("third");

        // List to get the actual order
        let sessions = app.sessions.list();
        let names: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();

        // Switch to session 1 (whatever it is in HashMap order)
        app.enter_command_mode();
        app.command_input = "switch 1".to_string();
        app.submit_input();
        let session = app.active_session().unwrap();
        assert_eq!(session.name, names[0]);

        // Switch to session 3
        app.enter_command_mode();
        app.command_input = "switch 3".to_string();
        app.submit_input();
        let session = app.active_session().unwrap();
        assert_eq!(session.name, names[2]);
    }

    #[test]
    fn test_command_switch_by_name() {
        let mut app = make_app();
        app.create_session("auth-fix");
        app.create_session("refactor");

        app.enter_command_mode();
        app.command_input = "switch auth".to_string();
        app.submit_input();
        let session = app.active_session().unwrap();
        assert_eq!(session.name, "auth-fix");
    }

    #[test]
    fn test_command_switch_invalid_index() {
        let mut app = make_app();
        app.create_session("test");
        app.enter_command_mode();
        app.command_input = "switch 5".to_string();
        app.submit_input();
        assert!(app.status.contains("Invalid index"));
    }

    #[test]
    fn test_command_close() {
        let mut app = make_app();
        app.create_session("test");
        assert!(app.active_session.is_some());

        app.enter_command_mode();
        app.command_input = "close".to_string();
        app.submit_input();
        assert!(app.active_session.is_none());
        assert!(app.messages.is_empty());
    }

    #[test]
    fn test_command_clear() {
        let mut app = make_app();
        app.add_message(MessageRole::User, "test");
        assert_eq!(app.messages.len(), 1);

        app.enter_command_mode();
        app.command_input = "clear".to_string();
        app.submit_input();
        assert!(app.messages.is_empty());
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn test_command_model() {
        let mut app = make_app();
        app.create_session("test");

        // Set model
        app.enter_command_mode();
        app.command_input = "model gpt-4o".to_string();
        app.submit_input();
        let session = app.active_session().unwrap();
        assert_eq!(session.model_id, "gpt-4o");

        // View model
        app.enter_command_mode();
        app.command_input = "model".to_string();
        app.submit_input();
        assert!(app.status.contains("gpt-4o"));
    }

    #[test]
    fn test_command_usage() {
        let mut app = make_app();
        app.token_display.input_tokens = 1000;
        app.token_display.output_tokens = 500;
        app.token_display.cache_tokens = 200;
        app.token_display.cost_usd = 0.05;

        app.enter_command_mode();
        app.command_input = "usage".to_string();
        app.submit_input();
        assert!(app.status.contains("1000"));
        assert!(app.status.contains("0.0500"));
    }

    #[test]
    fn test_switch_to_session_by_index() {
        let mut app = make_app();
        app.create_session("a");
        app.create_session("b");

        // Get actual order from HashMap
        let sessions = app.sessions.list();
        let first_name = sessions[0].name.clone();
        let second_name = sessions[1].name.clone();

        app.switch_to_session_by_index(1);
        assert_eq!(app.active_session().unwrap().name, first_name);

        app.switch_to_session_by_index(2);
        assert_eq!(app.active_session().unwrap().name, second_name);

        // Out of range
        app.switch_to_session_by_index(0);
        assert!(app.status.contains("Invalid"));

        app.switch_to_session_by_index(10);
        assert!(app.status.contains("Invalid"));
    }

    #[test]
    fn test_active_session_mut() {
        let mut app = make_app();
        app.create_session("test");
        let session = app.active_session_mut().unwrap();
        session.model_id = "new-model".to_string();
        assert_eq!(app.active_session().unwrap().model_id, "new-model");
    }

    // === AgentMode tests ===

    #[test]
    fn test_agent_mode_default() {
        let app = make_app();
        assert_eq!(app.agent_mode, AgentMode::Act);
    }

    #[test]
    fn test_set_agent_mode() {
        let mut app = make_app();
        app.set_agent_mode(AgentMode::Plan);
        assert_eq!(app.agent_mode, AgentMode::Plan);
        assert!(app.status.contains("Plan"));

        app.set_agent_mode(AgentMode::Auto);
        assert_eq!(app.agent_mode, AgentMode::Auto);
        assert!(app.status.contains("Auto"));
    }

    #[test]
    fn test_toggle_plan_act() {
        let mut app = make_app();
        assert_eq!(app.agent_mode, AgentMode::Act);

        // Act -> Plan
        app.toggle_plan_act();
        assert_eq!(app.agent_mode, AgentMode::Plan);

        // Plan -> Act
        app.toggle_plan_act();
        assert_eq!(app.agent_mode, AgentMode::Act);

        // Auto -> Plan
        app.set_agent_mode(AgentMode::Auto);
        app.toggle_plan_act();
        assert_eq!(app.agent_mode, AgentMode::Plan);
    }

    #[test]
    fn test_agent_mode_labels() {
        assert_eq!(AgentMode::Plan.label(), "Plan");
        assert_eq!(AgentMode::Act.label(), "Act");
        assert_eq!(AgentMode::Auto.label(), "Auto");
    }

    #[test]
    fn test_agent_mode_system_suffix() {
        let mut app = make_app();

        // Act mode — no suffix
        assert!(app.agent_mode_system_suffix().is_none());

        // Plan mode — has suffix about no tools
        app.set_agent_mode(AgentMode::Plan);
        let suffix = app.agent_mode_system_suffix().unwrap();
        assert!(suffix.contains("PLAN MODE"));
        assert!(suffix.contains("MUST NOT"));

        // Auto mode — has suffix about planning first
        app.set_agent_mode(AgentMode::Auto);
        let suffix = app.agent_mode_system_suffix().unwrap();
        assert!(suffix.contains("AUTO MODE"));
    }

    #[test]
    fn test_command_plan() {
        let mut app = make_app();
        app.enter_command_mode();
        app.command_input = "plan".to_string();
        app.submit_input();
        assert_eq!(app.agent_mode, AgentMode::Plan);
    }

    #[test]
    fn test_command_act() {
        let mut app = make_app();
        app.set_agent_mode(AgentMode::Plan);
        app.enter_command_mode();
        app.command_input = "act".to_string();
        app.submit_input();
        assert_eq!(app.agent_mode, AgentMode::Act);
    }

    #[test]
    fn test_command_auto() {
        let mut app = make_app();
        app.enter_command_mode();
        app.command_input = "auto".to_string();
        app.submit_input();
        assert_eq!(app.agent_mode, AgentMode::Auto);
    }

    // === Goal tests ===

    #[test]
    fn test_goal_default_none() {
        let app = make_app();
        assert!(app.goal.is_none());
        assert!(!app.is_goal_active());
    }

    #[test]
    fn test_create_goal() {
        let mut app = make_app();
        app.create_goal("fix auth bug", Some(10000));
        assert!(app.goal.is_some());
        let goal = app.goal.as_ref().unwrap();
        assert_eq!(goal.objective, "fix auth bug");
        assert_eq!(goal.status, GoalStatus::Active);
        assert_eq!(goal.token_budget, Some(10000));
        assert_eq!(goal.tokens_used, 0);
        assert!(app.is_goal_active());
    }

    #[test]
    fn test_goal_status_display() {
        let mut app = make_app();
        assert_eq!(app.goal_status_display(), "No active goal");

        app.create_goal("refactor db", None);
        let display = app.goal_status_display();
        assert!(display.contains("active"));
        assert!(display.contains("refactor db"));
        assert!(display.contains("Tokens: 0"));
    }

    #[test]
    fn test_pause_goal() {
        let mut app = make_app();
        app.create_goal("test goal", None);

        app.pause_goal();
        assert_eq!(app.goal.as_ref().unwrap().status, GoalStatus::Paused);
        assert!(!app.is_goal_active());

        // Cannot pause again
        app.pause_goal();
        assert!(app.status.contains("Cannot pause"));
    }

    #[test]
    fn test_pause_goal_none() {
        let mut app = make_app();
        app.pause_goal();
        assert_eq!(app.status, "No active goal");
    }

    #[test]
    fn test_resume_goal() {
        let mut app = make_app();
        app.create_goal("test goal", None);
        app.pause_goal();

        app.resume_goal();
        assert_eq!(app.goal.as_ref().unwrap().status, GoalStatus::Active);
        assert!(app.is_goal_active());
    }

    #[test]
    fn test_resume_goal_none() {
        let mut app = make_app();
        app.resume_goal();
        assert_eq!(app.status, "No active goal");
    }

    #[test]
    fn test_set_goal_budget() {
        let mut app = make_app();
        app.create_goal("test goal", None);

        app.set_goal_budget(50000);
        assert_eq!(app.goal.as_ref().unwrap().token_budget, Some(50000));
        assert!(app.status.contains("50000"));
    }

    #[test]
    fn test_set_goal_budget_none() {
        let mut app = make_app();
        app.set_goal_budget(50000);
        assert_eq!(app.status, "No active goal");
    }

    #[test]
    fn test_clear_goal() {
        let mut app = make_app();
        app.create_goal("test goal", None);
        assert!(app.goal.is_some());

        app.clear_goal();
        assert!(app.goal.is_none());
        assert!(app.status.contains("cleared"));
    }

    #[test]
    fn test_update_goal_tokens() {
        let mut app = make_app();
        app.create_goal("test goal", Some(1000));

        let exhausted = app.update_goal_tokens(500);
        assert!(!exhausted);
        assert_eq!(app.goal.as_ref().unwrap().tokens_used, 500);

        let exhausted = app.update_goal_tokens(500);
        assert!(exhausted);
        assert_eq!(app.goal.as_ref().unwrap().status, GoalStatus::BudgetLimited);
        assert!(!app.is_goal_active());
    }

    #[test]
    fn test_update_goal_tokens_no_budget() {
        let mut app = make_app();
        app.create_goal("test goal", None);

        let exhausted = app.update_goal_tokens(10000);
        assert!(!exhausted);
        assert_eq!(app.goal.as_ref().unwrap().tokens_used, 10000);
    }

    #[test]
    fn test_complete_goal() {
        let mut app = make_app();
        app.create_goal("test goal", None);

        app.complete_goal();
        assert_eq!(app.goal.as_ref().unwrap().status, GoalStatus::Complete);
        assert!(!app.is_goal_active());
        assert!(app.status.contains("completed"));
    }

    #[test]
    fn test_command_goal_create() {
        let mut app = make_app();
        app.enter_command_mode();
        app.command_input = "goal fix auth bug".to_string();
        app.submit_input();
        assert!(app.goal.is_some());
        assert_eq!(app.goal.as_ref().unwrap().objective, "fix auth bug");
    }

    #[test]
    fn test_command_goal_status() {
        let mut app = make_app();
        app.create_goal("test", None);
        app.enter_command_mode();
        app.command_input = "goal status".to_string();
        app.submit_input();
        assert!(app.status.contains("active"));
    }

    #[test]
    fn test_command_goal_pause_resume() {
        let mut app = make_app();
        app.create_goal("test", None);

        app.enter_command_mode();
        app.command_input = "goal pause".to_string();
        app.submit_input();
        assert_eq!(app.goal.as_ref().unwrap().status, GoalStatus::Paused);

        app.enter_command_mode();
        app.command_input = "goal resume".to_string();
        app.submit_input();
        assert_eq!(app.goal.as_ref().unwrap().status, GoalStatus::Active);
    }

    #[test]
    fn test_command_goal_budget() {
        let mut app = make_app();
        app.create_goal("test", None);

        app.enter_command_mode();
        app.command_input = "goal budget 25000".to_string();
        app.submit_input();
        assert_eq!(app.goal.as_ref().unwrap().token_budget, Some(25000));
    }

    #[test]
    fn test_command_goal_clear() {
        let mut app = make_app();
        app.create_goal("test", None);

        app.enter_command_mode();
        app.command_input = "goal clear".to_string();
        app.submit_input();
        assert!(app.goal.is_none());
    }

    #[test]
    fn test_command_goal_no_args() {
        let mut app = make_app();
        app.enter_command_mode();
        app.command_input = "goal".to_string();
        app.submit_input();
        assert_eq!(app.status, "No active goal");
    }

    #[test]
    fn test_format_elapsed_time() {
        assert_eq!(format_elapsed_time(0), "0s");
        assert_eq!(format_elapsed_time(59), "59s");
        assert_eq!(format_elapsed_time(60), "1m");
        assert_eq!(format_elapsed_time(3599), "59m");
        assert_eq!(format_elapsed_time(3600), "1h 0m");
        assert_eq!(format_elapsed_time(3661), "1h 1m");
    }

    // === Cat mascot tests ===

    #[test]
    fn test_cat_default_enabled() {
        let app = make_app();
        assert!(app.cat_enabled);
        assert_eq!(app.cat_state, CatState::Idle);
    }

    #[test]
    fn test_cat_state_ascii_art() {
        assert!(CatState::Idle.ascii_art().contains("^‥^"));
        assert!(CatState::Thinking.ascii_art().contains("^..^"));
        assert!(CatState::Executing.ascii_art().contains("｀^´"));
        assert!(CatState::Error.ascii_art().contains("；ェ；"));
        assert!(CatState::Done.ascii_art().contains("^‥^"));
    }

    #[test]
    fn test_cat_state_labels() {
        assert_eq!(CatState::Idle.label(), "sleeping");
        assert_eq!(CatState::Thinking.label(), "thinking");
        assert_eq!(CatState::Executing.label(), "working");
        assert_eq!(CatState::Error.label(), "surprised");
        assert_eq!(CatState::Done.label(), "happy");
    }

    #[test]
    fn test_set_cat_state() {
        let mut app = make_app();
        app.set_cat_state(CatState::Thinking);
        assert_eq!(app.cat_state, CatState::Thinking);
        app.set_cat_state(CatState::Executing);
        assert_eq!(app.cat_state, CatState::Executing);
    }

    #[test]
    fn test_cat_art_enabled() {
        let mut app = make_app();
        app.cat_enabled = true;
        app.set_cat_state(CatState::Idle);
        assert!(app.cat_art().contains("\\_/"));

        app.set_cat_state(CatState::Error);
        assert!(app.cat_art().contains("\\_/"));
    }

    #[test]
    fn test_cat_art_disabled() {
        let mut app = make_app();
        app.cat_enabled = false;
        assert_eq!(app.cat_art(), "");
    }

    #[test]
    fn test_command_cat_toggle() {
        let mut app = make_app();
        assert!(app.cat_enabled);

        // /cat off
        app.enter_command_mode();
        app.command_input = "cat off".to_string();
        app.submit_input();
        assert!(!app.cat_enabled);

        // /cat on
        app.enter_command_mode();
        app.command_input = "cat on".to_string();
        app.submit_input();
        assert!(app.cat_enabled);
    }

    #[test]
    fn test_command_cat_no_args_toggles() {
        let mut app = make_app();
        assert!(app.cat_enabled);

        app.enter_command_mode();
        app.command_input = "cat".to_string();
        app.submit_input();
        assert!(!app.cat_enabled);

        app.enter_command_mode();
        app.command_input = "cat".to_string();
        app.submit_input();
        assert!(app.cat_enabled);
    }

    // === Benchmark tests ===

    #[test]
    fn test_benchmark_default_cases() {
        let app = make_app();
        assert_eq!(app.benchmark_cases.len(), 5);
        assert!(app.benchmark_cases.iter().any(|c| c.id == "hello-world"));
    }

    #[test]
    fn test_benchmark_reports_empty() {
        let app = make_app();
        assert!(app.benchmark_reports.is_empty());
    }

    #[test]
    fn test_add_benchmark_report() {
        let mut app = make_app();
        let report = BenchmarkReport::from_results(
            "test",
            "model",
            vec![catcode_daemon::BenchmarkResult {
                case_id: "a".to_string(),
                provider_id: "test".to_string(),
                model_id: "model".to_string(),
                passed: true,
                input_tokens: 100,
                output_tokens: 50,
                cache_tokens: 0,
                latency_ms: 200,
                cost_usd: 0.001,
                output_preview: String::new(),
                error: None,
            }],
        );
        app.add_benchmark_report(report);
        assert_eq!(app.benchmark_reports.len(), 1);
        assert!(app.status.contains("1/1 passed"));
    }

    #[test]
    fn test_latest_benchmark_display_empty() {
        let app = make_app();
        assert_eq!(app.latest_benchmark_display(), "No benchmark results yet");
    }

    #[test]
    fn test_latest_benchmark_display_with_report() {
        let mut app = make_app();
        let report = BenchmarkReport::from_results("test", "model", vec![]);
        app.add_benchmark_report(report);
        let display = app.latest_benchmark_display();
        assert!(display.contains("=== test/model ==="));
    }

    #[test]
    fn test_command_benchmark_no_args() {
        let mut app = make_app();
        app.enter_command_mode();
        app.command_input = "benchmark".to_string();
        app.submit_input();
        assert!(app.status.contains("5 cases loaded"));
    }

    #[test]
    fn test_command_benchmark_list() {
        let mut app = make_app();
        app.enter_command_mode();
        app.command_input = "benchmark list".to_string();
        app.submit_input();
        assert!(app.status.contains("hello-world"));
        assert!(app.status.contains("fibonacci"));
    }

    #[test]
    fn test_command_benchmark_results_empty() {
        let mut app = make_app();
        app.enter_command_mode();
        app.command_input = "benchmark results".to_string();
        app.submit_input();
        assert!(app.status.contains("No benchmark results"));
    }

    #[test]
    fn test_command_benchmark_results_with_data() {
        let mut app = make_app();
        let report = BenchmarkReport::from_results("anthropic", "claude-sonnet-4", vec![]);
        app.add_benchmark_report(report);

        app.enter_command_mode();
        app.command_input = "benchmark results".to_string();
        app.submit_input();
        assert!(app.status.contains("anthropic"));
    }

    #[test]
    fn test_command_benchmark_clear() {
        let mut app = make_app();
        let report = BenchmarkReport::from_results("test", "model", vec![]);
        app.add_benchmark_report(report);
        assert_eq!(app.benchmark_reports.len(), 1);

        app.enter_command_mode();
        app.command_input = "benchmark clear".to_string();
        app.submit_input();
        assert!(app.benchmark_reports.is_empty());
        assert!(app.status.contains("cleared"));
    }

    #[test]
    fn test_input_history_saved_on_submit() {
        let mut app = make_app();
        app.input = "first message".to_string();
        app.input_cursor = 13;
        app.submit_input();

        app.input = "second message".to_string();
        app.input_cursor = 14;
        app.submit_input();

        assert_eq!(app.input_history, vec!["first message", "second message"]);
    }

    #[test]
    fn test_input_history_no_duplicate() {
        let mut app = make_app();
        app.input = "same".to_string();
        app.input_cursor = 4;
        app.submit_input();

        app.input = "same".to_string();
        app.input_cursor = 4;
        app.submit_input();

        assert_eq!(app.input_history.len(), 1);
    }

    #[test]
    fn test_history_up_navigation() {
        let mut app = make_app();
        app.input = "first".to_string();
        app.input_cursor = 5;
        app.submit_input();

        app.input = "second".to_string();
        app.input_cursor = 6;
        app.submit_input();

        // Navigate up
        app.history_up();
        assert_eq!(app.input, "second");
        assert_eq!(app.input_cursor, 6);

        app.history_up();
        assert_eq!(app.input, "first");
        assert_eq!(app.input_cursor, 5);
    }

    #[test]
    fn test_history_down_navigation() {
        let mut app = make_app();
        app.input = "first".to_string();
        app.input_cursor = 5;
        app.submit_input();

        app.input = "second".to_string();
        app.input_cursor = 6;
        app.submit_input();

        // Go up twice
        app.history_up();
        app.history_up();
        assert_eq!(app.input, "first");

        // Go down once
        app.history_down();
        assert_eq!(app.input, "second");

        // Go down again restores saved input
        app.history_down();
        assert_eq!(app.input, "");
        assert!(app.history_index.is_none());
    }

    #[test]
    fn test_history_preserves_current_input() {
        let mut app = make_app();
        app.input = "old".to_string();
        app.input_cursor = 3;
        app.submit_input();

        app.input = "typing somethi".to_string();
        app.input_cursor = 14;

        // Navigate up
        app.history_up();
        assert_eq!(app.input, "old");

        // Navigate down restores what we were typing
        app.history_down();
        assert_eq!(app.input, "typing somethi");
    }
}
