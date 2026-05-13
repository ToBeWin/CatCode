//! # catcode-tui
//!
//! Terminal User Interface for the CatCode AI coding agent.
//!
//! Provides a ratatui-based TUI with:
//! - Session management panel (left)
//! - Chat messages display (center)
//! - Token/cost tracking (top bar)
//! - Input box with `/` command support (bottom)
//! - Keyboard shortcuts for common operations

/// The `app` module.
pub mod app;
/// The `event` module.
pub mod event;
/// The `ui` module.
pub mod ui;

pub use app::{AgentMode, App, InputMode};

use crossterm::{
    event::{KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

/// Run the TUI application.
///
/// Sets up the terminal, creates the app state, and enters the event loop.
/// The loop processes keyboard events and renders the UI until the user quits.
pub async fn run(project_dir: PathBuf) -> anyhow::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new(project_dir.clone());
    let mut event_handler = event::EventHandler::new(Duration::from_millis(250));

    // Try to restore sessions from database
    let db_path = project_dir.join(".catcode").join("catcode.db");
    let restored_count = {
        let mut count = 0u64;
        if let Ok(db) = catcode_daemon::Database::new(
            &db_path.to_string_lossy(),
        )
        .await
            && let Ok(sessions) = db.list_sessions().await
        {
            for row in &sessions {
                let state = row.parse_state();
                let is_terminal = matches!(
                    state,
                    catcode_daemon::SessionState::Completed
                        | catcode_daemon::SessionState::Failed(_)
                );
                if !is_terminal {
                    let session = catcode_daemon::Session::new(
                        &row.name,
                        std::path::PathBuf::from(&row.project_dir),
                        &row.model_id,
                        &row.provider_id,
                    );
                    let id = session.id.clone();
                    app.sessions.force_remove(&id);
                    app.sessions.force_add(session);
                    if let Some(s) = app.sessions.get_mut(&id) {
                        s.set_state(state);
                        s.turn_count = row.turn_count as u64;
                    }
                    count += 1;
                }
            }
        }
        count
    };

    // Check if config exists for welcome message
    let config_exists = project_dir.join(".catcode").join("config.toml").exists()
        || std::path::PathBuf::from("./catcode.toml").exists()
        || dirs::config_dir()
            .map(|p| p.join("catcode").join("config.toml").exists())
            .unwrap_or(false);

    // Create a default session if none restored
    if app.sessions.total_count() == 0 {
        app.create_session("main");
    }

    // Show appropriate welcome message
    let welcome = if config_exists {
        if restored_count > 0 {
            format!(
                "Welcome back! Restored {} session{} from last session. Type a message or use /help for commands.",
                restored_count,
                if restored_count == 1 { "" } else { "s" },
            )
        } else {
            "Welcome to CatCode! Type a message or use /help for commands.".to_string()
        }
    } else {
        if restored_count > 0 {
            app.add_message(
                app::MessageRole::System,
                format!("Restored {} session{} from last session.", restored_count, if restored_count == 1 { "" } else { "s" }),
            );
        }
        let welcome_box = "\
╔══════════════════════════════════════════╗
║         Welcome to CatCode!             ║
║                                          ║
║  Quick start:                            ║
║   1. /set-provider <name>  — set provider║
║   2. /model <name>        — choose model ║
║   3. Type your message and press Enter   ║
║                                          ║
║  Commands: /help  to see all commands    ║
║  First time?  Run: catcode init          ║
╚══════════════════════════════════════════╝";
        welcome_box.to_string()
    };
    app.add_message(app::MessageRole::System, &welcome);

    // Main event loop
    let result = run_event_loop(&mut terminal, &mut app, &mut event_handler).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// The main event loop, separated for testability.
async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    event_handler: &mut event::EventHandler,
) -> anyhow::Result<()> {
    // Initialize agent communication channel
    app.init_agent_channel();

    loop {
        // Poll agent events before rendering
        app.poll_agent_events();

        // Render
        terminal.draw(|f| ui::render(f, app))?;

        // Handle events
        match event_handler.next().await {
            event::AppEvent::Key(key) => {
                handle_key_event(app, key);
            }
            event::AppEvent::Resize(_, _) => {
                // Terminal was resized — ratatui handles this automatically
            }
            event::AppEvent::Tick => {
                app.tick();
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

/// Handle a single key event.
fn handle_key_event(app: &mut App, key: KeyEvent) {
    // Global shortcuts (work in all modes)
    if event::is_ctrl_key(&key, KeyCode::Char('q')) {
        app.quit();
        return;
    }

    if event::is_ctrl_key(&key, KeyCode::Char('c')) {
        if app.input_mode == InputMode::Command {
            app.enter_normal_mode();
        } else {
            app.quit();
        }
        return;
    }

    match app.input_mode {
        InputMode::Normal => handle_normal_key(app, key),
        InputMode::Command => handle_command_key(app, key),
    }
}

/// Handle key events in normal (message input) mode.
fn handle_normal_key(app: &mut App, key: KeyEvent) {
    match key.code {
        // Enter — submit message
        KeyCode::Enter => {
            if let Some(text) = app.submit_input() {
                app.send_to_agent(&text);
            }
        }
        // Backspace
        KeyCode::Backspace => {
            app.handle_backspace();
        }
        // Tab — switch to command mode (only without Ctrl)
        KeyCode::Tab if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.enter_command_mode();
        }
        // Ctrl+N — new session
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.create_session("new-session");
        }
        // Ctrl+W — close session
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(id) = app.active_session.clone() {
                let _ = app
                    .sessions
                    .update_state(&id, catcode_daemon::SessionState::Completed);
                app.active_session = None;
                app.messages.clear();
                app.status = "Session closed".to_string();
            }
        }
        // Ctrl+K — clear messages
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.messages.clear();
            app.scroll_offset = 0;
            app.status = "Messages cleared".to_string();
        }
        // Ctrl+L — clear input
        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.input.clear();
            app.input_cursor = 0;
        }
        // Ctrl+P — toggle plan/act mode
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.toggle_plan_act();
        }
        // Ctrl+1-9 — switch to session by number
        KeyCode::Char(c @ '1'..='9') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let index = c as usize - '0' as usize;
            app.switch_to_session_by_index(index);
        }
        // Ctrl+Right — cycle to next session
        KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let sessions = app.sessions.list();
            if sessions.len() > 1 {
                let current_idx = app
                    .active_session
                    .as_ref()
                    .and_then(|id| sessions.iter().position(|s| &s.id == id))
                    .unwrap_or(0);
                let next_idx = (current_idx + 1) % sessions.len();
                app.active_session = Some(sessions[next_idx].id.clone());
                app.messages.clear();
                app.scroll_offset = 0;
                app.status = format!("Switched to: {}", sessions[next_idx].name);
            }
        }
        // Ctrl+Left — cycle to previous session
        KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let sessions = app.sessions.list();
            if sessions.len() > 1 {
                let current_idx = app
                    .active_session
                    .as_ref()
                    .and_then(|id| sessions.iter().position(|s| &s.id == id))
                    .unwrap_or(0);
                let prev_idx = if current_idx == 0 {
                    sessions.len() - 1
                } else {
                    current_idx - 1
                };
                app.active_session = Some(sessions[prev_idx].id.clone());
                app.messages.clear();
                app.scroll_offset = 0;
                app.status = format!("Switched to: {}", sessions[prev_idx].name);
            }
        }
        // Left — move cursor left
        KeyCode::Left if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.input_cursor > 0 {
                let prev = app.input[..app.input_cursor]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                app.input_cursor = prev;
            }
        }
        // Right — move cursor right
        KeyCode::Right if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.input_cursor < app.input.len() {
                let next = app.input[app.input_cursor..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| app.input_cursor + i)
                    .unwrap_or(app.input.len());
                app.input_cursor = next;
            }
        }
        // Page Up — scroll up
        KeyCode::PageUp => {
            app.scroll_up(10);
        }
        // Page Down — scroll down
        KeyCode::PageDown => {
            app.scroll_down(10);
        }
        // Ctrl+Up/Down — input history navigation
        KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.history_up();
        }
        KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.history_down();
        }
        // Up/Down — scroll messages
        KeyCode::Up => {
            app.scroll_up(1);
        }
        KeyCode::Down => {
            app.scroll_down(1);
        }
        // Home — scroll to top
        KeyCode::Home => {
            app.scroll_offset = 0;
        }
        // End — scroll to bottom
        KeyCode::End => {
            app.scroll_to_bottom();
        }
        // / — enter command mode
        KeyCode::Char('/') => {
            app.enter_command_mode();
        }
        // Regular character
        KeyCode::Char(c) => {
            app.handle_char(c);
        }
        _ => {}
    }
}

/// Handle key events in command mode.
fn handle_command_key(app: &mut App, key: KeyEvent) {
    match key.code {
        // Enter — execute command
        KeyCode::Enter => {
            app.submit_input();
        }
        // Escape — cancel command
        KeyCode::Esc => {
            app.enter_normal_mode();
        }
        // Backspace
        KeyCode::Backspace => {
            app.handle_backspace();
        }
        // Regular character
        KeyCode::Char(c) => {
            app.handle_char(c);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn test_handle_key_quit() {
        let mut app = App::new(PathBuf::from("/tmp"));
        handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL),
        );
        assert!(app.should_quit);
    }

    #[test]
    fn test_handle_key_slash_enters_command() {
        let mut app = App::new(PathBuf::from("/tmp"));
        handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        );
        assert_eq!(app.input_mode, InputMode::Command);
    }

    #[test]
    fn test_handle_key_esc_exits_command() {
        let mut app = App::new(PathBuf::from("/tmp"));
        app.enter_command_mode();
        handle_key_event(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_handle_key_enter_submits() {
        let mut app = App::new(PathBuf::from("/tmp"));
        app.input = "hello".to_string();
        app.input_cursor = 5;
        handle_key_event(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.input.is_empty());
        assert_eq!(app.messages.len(), 1);
    }

    #[test]
    fn test_handle_key_char_types() {
        let mut app = App::new(PathBuf::from("/tmp"));
        handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        );
        assert_eq!(app.input, "a");
    }

    #[test]
    fn test_handle_key_ctrl_n_new_session() {
        let mut app = App::new(PathBuf::from("/tmp"));
        handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
        );
        assert!(app.active_session.is_some());
    }

    #[test]
    fn test_handle_key_page_scroll() {
        let mut app = App::new(PathBuf::from("/tmp"));
        // Start with some scroll offset (simulating being scrolled down)
        app.scroll_offset = 20;
        handle_key_event(&mut app, KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(app.scroll_offset, 10); // scrolled up by 10
        handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
        );
        assert_eq!(app.scroll_offset, 20); // scrolled back down
    }

    #[test]
    fn test_handle_key_ctrl_k_clear_messages() {
        let mut app = App::new(PathBuf::from("/tmp"));
        app.add_message(app::MessageRole::User, "test");
        assert_eq!(app.messages.len(), 1);

        handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
        );
        assert!(app.messages.is_empty());
    }

    #[test]
    fn test_handle_key_ctrl_l_clear_input() {
        let mut app = App::new(PathBuf::from("/tmp"));
        app.input = "hello world".to_string();
        app.input_cursor = 11;

        handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL),
        );
        assert!(app.input.is_empty());
        assert_eq!(app.input_cursor, 0);
    }

    #[test]
    fn test_handle_key_ctrl_number_switch_session() {
        let mut app = App::new(PathBuf::from("/tmp"));
        app.create_session("first");
        app.create_session("second");
        app.create_session("third");

        // Get actual order from HashMap
        let sessions = app.sessions.list();
        let first_name = sessions[0].name.clone();
        let third_name = sessions[2].name.clone();

        // Ctrl+1 -> first session
        handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.active_session().unwrap().name, first_name);

        // Ctrl+3 -> third session
        handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('3'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.active_session().unwrap().name, third_name);
    }

    #[test]
    fn test_handle_key_home_end_scroll() {
        let mut app = App::new(PathBuf::from("/tmp"));
        app.scroll_offset = 50;

        handle_key_event(&mut app, KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        assert_eq!(app.scroll_offset, 0);

        handle_key_event(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(app.scroll_offset, usize::MAX);
    }

    #[test]
    fn test_handle_key_ctrl_p_toggle_mode() {
        let mut app = App::new(PathBuf::from("/tmp"));
        assert_eq!(app.agent_mode, app::AgentMode::Act);

        // Ctrl+P -> Plan
        handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.agent_mode, app::AgentMode::Plan);

        // Ctrl+P -> Act
        handle_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.agent_mode, app::AgentMode::Act);
    }
}
