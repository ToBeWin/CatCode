use crate::app::{AgentMode, App, CatState, GoalStatus, InputMode, MessageRole};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

/// All available commands for the command palette.
const COMMANDS: &[(&str, &str)] = &[
    ("new <name>", "Create a new session"),
    ("sessions", "List all sessions"),
    ("switch <n|name>", "Switch to session"),
    ("close", "Close current session"),
    ("clear", "Clear messages"),
    ("model <name>", "Set/view model"),
    ("usage", "Show token usage"),
    ("plan", "Enter plan mode (no tools)"),
    ("act", "Enter act mode (default)"),
    ("auto", "Plan first, then execute"),
    ("goal <objective>", "Create autonomous goal"),
    ("goal status", "Show goal status"),
    ("goal pause", "Pause active goal"),
    ("goal resume", "Resume paused goal"),
    ("goal clear", "Clear current goal"),
    ("cat on|off", "Toggle cat mascot"),
    ("benchmark list", "List benchmark cases"),
    ("benchmark results", "Show benchmark results"),
    ("benchmark clear", "Clear results"),
    ("quit", "Exit CatCode"),
];

/// Render the main UI.
pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top bar
            Constraint::Min(10),   // Main content
            Constraint::Length(3), // Input area
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    render_top_bar(f, app, chunks[0]);
    render_main_area(f, app, chunks[1]);
    render_input(f, app, chunks[2]);
    render_status_bar(f, app, chunks[3]);

    // Command suggestions overlay (when in command mode)
    if app.input_mode == InputMode::Command {
        render_command_suggestions(f, app);
    }
}

/// Render command suggestions overlay when in command mode.
fn render_command_suggestions(f: &mut Frame, app: &App) {
    let input = app.command_input.to_lowercase();
    let suggestions: Vec<(&str, &str)> = COMMANDS
        .iter()
        .filter(|(cmd, _)| {
            let cmd_name = cmd.split_whitespace().next().unwrap_or(cmd);
            input.is_empty() || cmd_name.starts_with(&input)
        })
        .copied()
        .collect();

    if suggestions.is_empty() {
        return;
    }

    // Calculate popup size
    let popup_height = (suggestions.len() as u16 + 2).min(12); // +2 for borders
    let popup_width = 40u16.min(f.area().width.saturating_sub(4));

    // Position: centered horizontally, above input area
    let input_area_y = f.area().height.saturating_sub(4); // input starts here
    let popup_y = input_area_y.saturating_sub(popup_height + 1);
    let popup_x = (f.area().width.saturating_sub(popup_width)) / 2;

    let area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    // Clear the area behind the popup
    f.render_widget(Clear, area);

    let items: Vec<ListItem> = suggestions
        .iter()
        .map(|(cmd, desc)| {
            let cmd_name = cmd.split_whitespace().next().unwrap_or(cmd);
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("/{:<14}", cmd_name),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {}", desc),
                    Style::default().fg(Color::Gray),
                ),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Commands ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .style(Style::default().bg(Color::Black)),
    );

    f.render_widget(list, area);
}

/// Render the top bar with session name, model, and token info.
fn render_top_bar(f: &mut Frame, app: &App, area: Rect) {
    let session_name = app
        .active_session()
        .map(|s| s.name.as_str())
        .unwrap_or("no session");

    let model = app
        .active_session()
        .map(|s| s.model_id.as_str())
        .unwrap_or("-");

    let tokens = format!(
        "In:{} Out:{} Cache:{} ${:.4}",
        app.token_display.input_tokens,
        app.token_display.output_tokens,
        app.token_display.cache_tokens,
        app.token_display.cost_usd,
    );

    let mode_label = app.agent_mode.label();
    let mode_color = match app.agent_mode {
        AgentMode::Plan => Color::Magenta,
        AgentMode::Act => Color::Green,
        AgentMode::Auto => Color::Yellow,
    };

    let mut spans = vec![
        Span::styled(
            " CatCode ".to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" [{}]", mode_label),
            Style::default()
                .fg(mode_color)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    // Show goal indicator if there's an active goal
    if let Some(goal) = &app.goal {
        let (goal_icon, goal_color) = match goal.status {
            GoalStatus::Active => ("GOAL", Color::Green),
            GoalStatus::Paused => ("GOAL⏸", Color::Yellow),
            GoalStatus::BudgetLimited => ("GOAL$", Color::Red),
            GoalStatus::Complete => ("GOAL✓", Color::Blue),
        };
        spans.push(Span::styled(
            format!(" [{}]", goal_icon),
            Style::default().fg(goal_color).add_modifier(Modifier::BOLD),
        ));
    }

    spans.push(Span::raw(format!(" [{}]", session_name)));
    spans.push(Span::styled(format!(" [{}]", model), Style::default().fg(Color::Yellow)));
    spans.push(Span::styled(format!(" {}", tokens), Style::default().fg(Color::Green)));

    // Cat mascot
    if app.cat_enabled {
        let cat_color = match app.cat_state {
            CatState::Idle => Color::DarkGray,
            CatState::Thinking => Color::Yellow,
            CatState::Executing => Color::Green,
            CatState::Error => Color::Red,
            CatState::Done => Color::Cyan,
        };
        spans.push(Span::styled(
            format!(" {}", app.cat_art()),
            Style::default().fg(cat_color),
        ));
    }

    let line = Line::from(spans);

    let paragraph = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    f.render_widget(paragraph, area);
}

/// Render the main content area with sessions list and messages.
fn render_main_area(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20), // Sessions panel
            Constraint::Min(40),    // Messages
        ])
        .split(area);

    render_sessions_panel(f, app, chunks[0]);
    render_messages(f, app, chunks[1]);
}

/// Render the sessions list panel.
fn render_sessions_panel(f: &mut Frame, app: &App, area: Rect) {
    let sessions = app.sessions.list();
    let count = sessions.len();

    let items: Vec<ListItem> = sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let indicator = match &s.state {
                SessionState::Running => Span::styled("●", Style::default().fg(Color::Green)),
                SessionState::Paused => Span::styled("◐", Style::default().fg(Color::Yellow)),
                SessionState::Completed => Span::styled("✓", Style::default().fg(Color::Blue)),
                SessionState::Failed(_) => Span::styled("✗", Style::default().fg(Color::Red)),
            };

            let is_active = app.active_session.as_ref() == Some(&s.id);
            let name_style = if is_active {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let num_style = Style::default().fg(Color::DarkGray);
            let num = if i < 9 {
                format!("{} ", i + 1)
            } else {
                "  ".to_string()
            };

            ListItem::new(Line::from(vec![
                Span::styled(num, num_style),
                indicator,
                Span::raw(" "),
                Span::styled(&s.name, name_style),
            ]))
        })
        .collect();

    let title = format!(" Sessions ({}) ", count);
    let list = List::new(items).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(list, area);
}

use catcode_daemon::SessionState;

/// Render the messages area.
fn render_messages(f: &mut Frame, app: &App, area: Rect) {
    let inner_height = area.height.saturating_sub(2) as usize; // subtract borders

    let lines: Vec<Line> = app
        .messages
        .iter()
        .flat_map(|msg| {
            let (prefix, style) = match msg.role {
                MessageRole::User => (
                    "You: ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                MessageRole::Assistant => (
                    "Agent: ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                MessageRole::System => (
                    "System: ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::ITALIC),
                ),
                MessageRole::Tool => (
                    "Tool: ",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
            };

            // Split content into lines
            let content_lines: Vec<&str> = msg.content.lines().collect();
            let mut result = Vec::new();

            for (i, line) in content_lines.iter().enumerate() {
                if i == 0 {
                    result.push(Line::from(vec![
                        Span::styled(prefix, style),
                        Span::raw(*line),
                    ]));
                } else {
                    result.push(Line::from(Span::raw(format!("  {}", line))));
                }
            }
            result.push(Line::from(Span::raw(""))); // blank line between messages
            result
        })
        .collect();

    // Calculate scroll offset
    let total_lines = lines.len();
    let scroll = if app.scroll_offset == usize::MAX {
        total_lines.saturating_sub(inner_height)
    } else {
        app.scroll_offset
            .min(total_lines.saturating_sub(inner_height))
    };

    let paragraph = Paragraph::new(lines)
        .scroll((scroll as u16, 0))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .title(" Chat ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

    f.render_widget(paragraph, area);
}

/// Render the input area.
fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let (title, style) = match app.input_mode {
        InputMode::Normal => (" Input ", Style::default().fg(Color::White)),
        InputMode::Command => (" Command ", Style::default().fg(Color::Yellow)),
    };
    let command_text = format!("/{}", app.command_input);
    let input_text = match app.input_mode {
        InputMode::Normal => app.input.as_str(),
        InputMode::Command => command_text.as_str(),
    };

    let input = Paragraph::new(input_text).style(style).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(
                Style::default().fg(if app.input_mode == InputMode::Command {
                    Color::Yellow
                } else {
                    Color::DarkGray
                }),
            ),
    );

    f.render_widget(input, area);
}

/// Render the status bar at the bottom.
fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let session_count = app.sessions.total_count();
    let help = match app.input_mode {
        InputMode::Normal => {
            let mode_hint = match app.agent_mode {
                AgentMode::Plan => "Ctrl+P:switch to act",
                AgentMode::Act => "Ctrl+P:plan mode",
                AgentMode::Auto => "Ctrl+P:plan mode",
            };
            if session_count > 1 {
                format!(" Enter:send | /:cmd | {} | Ctrl+1-9:switch | Ctrl+N:new", mode_hint)
            } else {
                format!(" Enter:send | /:cmd | {} | Ctrl+N:new", mode_hint)
            }
        }
        InputMode::Command => " Enter:exec | Esc:cancel | Tab:autocomplete".to_string(),
    };

    let status_text = if app.status.is_empty() {
        help
    } else {
        format!("{} │ {}", app.status, help)
    };

    let paragraph =
        Paragraph::new(status_text).style(Style::default().fg(Color::DarkGray).bg(Color::Black));

    f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_render_doesnt_panic() {
        // Basic smoke test — render with a default app and verify no panics
        let app = App::new(PathBuf::from("/tmp"));
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render(f, &app);
            })
            .unwrap();
    }

    #[test]
    fn test_render_with_sessions() {
        let mut app = App::new(PathBuf::from("/tmp"));
        app.create_session("test-1");
        app.create_session("test-2");
        app.add_message(MessageRole::User, "hello");
        app.add_message(MessageRole::Assistant, "hi there");

        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render(f, &app);
            })
            .unwrap();
    }

    #[test]
    fn test_render_in_command_mode() {
        let mut app = App::new(PathBuf::from("/tmp"));
        app.create_session("test");
        app.enter_command_mode();
        app.command_input = "he".to_string(); // should match "help"

        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render(f, &app);
            })
            .unwrap();
    }

    #[test]
    fn test_command_suggestions_filtering() {
        // Verify the command list is non-empty
        assert!(!COMMANDS.is_empty());
        assert!(COMMANDS.iter().any(|(cmd, _)| cmd.starts_with("new")));
        assert!(COMMANDS.iter().any(|(cmd, _)| cmd.starts_with("quit")));
    }
}
