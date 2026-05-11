use crate::app::{App, InputMode, MessageRole};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

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

    let line = Line::from(vec![
        Span::styled(
            " CatCode ".to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" [{}]", session_name)),
        Span::styled(format!(" [{}]", model), Style::default().fg(Color::Yellow)),
        Span::styled(format!(" {}", tokens), Style::default().fg(Color::Green)),
    ]);

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

    let items: Vec<ListItem> = sessions
        .iter()
        .map(|s| {
            let indicator = match &s.state {
                SessionState::Running => Span::styled("● ", Style::default().fg(Color::Green)),
                SessionState::Paused => Span::styled("◐ ", Style::default().fg(Color::Yellow)),
                SessionState::Completed => Span::styled("✓ ", Style::default().fg(Color::Blue)),
                SessionState::Failed(_) => Span::styled("✗ ", Style::default().fg(Color::Red)),
            };

            let is_active = app.active_session.as_ref() == Some(&s.id);
            let style = if is_active {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            ListItem::new(Line::from(vec![indicator, Span::styled(&s.name, style)]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Sessions ")
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
    let help = match app.input_mode {
        InputMode::Normal => " Enter:send | /:command | Ctrl+N:new | Ctrl+Q:quit",
        InputMode::Command => " Enter:execute | Esc:cancel",
    };

    let status_text = if app.status.is_empty() {
        help.to_string()
    } else {
        format!("{} | {}", app.status, help)
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
}
