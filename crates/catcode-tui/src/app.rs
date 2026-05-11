use catcode_daemon::{Session, SessionManager, SessionState};
use std::path::PathBuf;

/// Input mode for the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    /// Normal mode — typing messages to the agent.
    Normal,
    /// Command mode — typing a `/` command.
    Command,
}

/// A chat message displayed in the main content area.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
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
}

#[derive(Debug, Clone, Default)]
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
                self.input_cursor += 1;
            }
            InputMode::Command => {
                self.command_input.insert(self.command_input.len(), c);
            }
        }
    }

    /// Handle backspace.
    pub fn handle_backspace(&mut self) {
        match self.input_mode {
            InputMode::Normal => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    self.input.remove(self.input_cursor);
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
                self.input.clear();
                self.input_cursor = 0;
                self.add_message(MessageRole::User, &text);
                Some(text)
            }
            InputMode::Command => {
                let cmd = self.command_input.clone();
                self.command_input.clear();
                self.input_mode = InputMode::Normal;
                self.execute_command(&cmd);
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
                        .map(|s| {
                            let state = match &s.state {
                                SessionState::Running => "●",
                                SessionState::Paused => "◐",
                                SessionState::Completed => "✓",
                                SessionState::Failed(_) => "✗",
                            };
                            format!("{} {} ({})", state, s.name, &s.id[..8])
                        })
                        .collect();
                    self.status = names.join(" | ");
                }
            }
            "help" | "h" => {
                self.status = "/new <name> | /sessions | /quit | /help".to_string();
            }
            _ => {
                self.status = format!("Unknown command: {}", parts[0]);
            }
        }
    }

    /// Quit the application.
    pub fn quit(&mut self) {
        self.should_quit = true;
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
    fn test_handle_backspace() {
        let mut app = make_app();
        app.handle_char('a');
        app.handle_char('b');
        app.handle_backspace();
        assert_eq!(app.input, "a");
        assert_eq!(app.input_cursor, 1);
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
}
