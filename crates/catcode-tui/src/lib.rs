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

pub mod app;
pub mod event;
pub mod ui;

pub use app::{App, InputMode};

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
    let mut app = App::new(project_dir);
    let mut event_handler = event::EventHandler::new(Duration::from_millis(250));

    // Create a default session
    app.create_session("main");

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
    loop {
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
                // Periodic tick — could be used for animations or polling
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
            app.submit_input();
        }
        // Backspace
        KeyCode::Backspace => {
            app.handle_backspace();
        }
        // Tab — switch to command mode
        KeyCode::Tab => {
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
        // Page Up — scroll up
        KeyCode::PageUp => {
            app.scroll_up(10);
        }
        // Page Down — scroll down
        KeyCode::PageDown => {
            app.scroll_down(10);
        }
        // Up/Down — scroll
        KeyCode::Up => {
            app.scroll_up(1);
        }
        KeyCode::Down => {
            app.scroll_down(1);
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
}
