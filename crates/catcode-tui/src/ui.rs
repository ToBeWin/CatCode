use crate::app::{AgentMode, App, CatState, InputMode, MessageRole};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

const HEADER_BG: Color = Color::Rgb(30, 30, 50);
const ACCENT: Color = Color::Rgb(100, 180, 255);
const SUCCESS: Color = Color::Rgb(80, 200, 120);
const WARN: Color = Color::Rgb(255, 200, 50);
const ERROR: Color = Color::Rgb(255, 80, 80);
const TEXT: Color = Color::Rgb(220, 220, 220);
const DIM: Color = Color::Rgb(120, 120, 120);
const USER_MSG: Color = Color::Rgb(100, 180, 255);
const ASSISTANT_MSG: Color = Color::Rgb(80, 200, 120);
const SYSTEM_MSG: Color = Color::Rgb(255, 200, 50);
const TOOL_MSG: Color = Color::Rgb(180, 140, 200);

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

    if app.input_mode == InputMode::Command {
        render_command_suggestions(f, app);
    }
}

fn border_style(active: bool) -> Style {
    if active {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(DIM)
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

    let popup_height = (suggestions.len() as u16 + 2).min(12);
    let popup_width = 40u16.min(f.area().width.saturating_sub(4));
    let input_area_y = f.area().height.saturating_sub(4);
    let popup_y = input_area_y.saturating_sub(popup_height + 1);
    let popup_x = (f.area().width.saturating_sub(popup_width)) / 2;

    let area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, area);

    let items: Vec<ListItem> = suggestions
        .iter()
        .map(|(cmd, desc)| {
            let cmd_name = cmd.split_whitespace().next().unwrap_or(cmd);
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("/{:<14}", cmd_name),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {}", desc), Style::default().fg(DIM)),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Commands ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(WARN)),
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
        " {}in {}out ${:.4}",
        app.token_display.input_tokens,
        app.token_display.output_tokens,
        app.token_display.cost_usd,
    );

    let mode_label = app.agent_mode.label();
    let mode_color = match app.agent_mode {
        AgentMode::Plan => Color::Magenta,
        AgentMode::Act => SUCCESS,
        AgentMode::Auto => WARN,
    };

    let spans = vec![
        Span::styled(" ◆ ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(" {} ", mode_label),
            Style::default()
                .fg(mode_color)
                .bg(HEADER_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", session_name),
            Style::default().fg(TEXT).bg(HEADER_BG),
        ),
        Span::styled(
            format!(" {} ", model),
            Style::default().fg(WARN).bg(HEADER_BG),
        ),
        Span::styled(tokens, Style::default().fg(SUCCESS).bg(HEADER_BG)),
    ];

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(Style::default().bg(HEADER_BG));
    f.render_widget(paragraph, area);
}

/// Render the main content area with sessions list, thinking panel, and messages.
fn render_main_area(f: &mut Frame, app: &App, area: Rect) {
    if app.has_thinking() {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(20),
                Constraint::Percentage(30),
                Constraint::Min(40),
            ])
            .split(area);

        render_sessions_panel(f, app, chunks[0]);
        render_thinking_panel(f, app, chunks[1]);
        render_messages(f, app, chunks[2]);
    } else {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(20),
                Constraint::Min(40),
            ])
            .split(area);

        render_sessions_panel(f, app, chunks[0]);
        render_messages(f, app, chunks[1]);
    }

    render_cat_overlay(f, app, area);
}

/// Render the cat mascot as a floating overlay in the corner.
fn render_cat_overlay(f: &mut Frame, app: &App, area: Rect) {
    if !app.cat_enabled {
        return;
    }

    let cat_lines: Vec<&str> = app.cat_art().lines().collect();
    let cat_height = cat_lines.len() as u16;
    let cat_width = 12u16;

    if area.width < cat_width + 2 || area.height < cat_height + 1 {
        return;
    }

    let x = area.x + area.width.saturating_sub(cat_width + 2);
    let y = area.y + 1;

    let overlay_area = Rect { x, y, width: cat_width + 2, height: cat_height + 1 };

    f.render_widget(Clear, overlay_area);

    let cat_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(DIM));

    let inner = cat_block.inner(overlay_area);
    f.render_widget(cat_block, overlay_area);

    let cat_color = match app.cat_state {
        CatState::Idle => DIM,
        CatState::Thinking => WARN,
        CatState::Executing => SUCCESS,
        CatState::Error => ERROR,
        CatState::Done => ACCENT,
    };

    let lines: Vec<Line> = cat_lines
        .iter()
        .map(|line| Line::from(Span::styled(*line, Style::default().fg(cat_color))))
        .collect();

    let paragraph = Paragraph::new(lines)
        .style(Style::default().bg(Color::Black));

    f.render_widget(paragraph, inner);
}

/// Render the thinking panel showing real-time reasoning content.
fn render_thinking_panel(f: &mut Frame, app: &App, area: Rect) {
    let text = if app.current_thinking.is_empty() {
        " Waiting for thinking...".to_string()
    } else {
        app.current_thinking.clone()
    };

    let paragraph = Paragraph::new(text)
        .style(Style::default().fg(WARN).italic())
        .block(
            Block::default()
                .title(" Thinking... ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(WARN)),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

use catcode_daemon::SessionState;

/// Render the sessions list panel.
fn render_sessions_panel(f: &mut Frame, app: &App, area: Rect) {
    let sessions = app.sessions.list();
    let count = sessions.len();

    let items: Vec<ListItem> = sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let indicator = match &s.state {
                SessionState::Running => Span::styled("●", Style::default().fg(SUCCESS)),
                SessionState::Paused => Span::styled("◐", Style::default().fg(WARN)),
                SessionState::Completed => Span::styled("✓", Style::default().fg(Color::Blue)),
                SessionState::Failed(_) => Span::styled("✗", Style::default().fg(ERROR)),
            };

            let is_active = app.active_session.as_ref() == Some(&s.id);
            let name_style = if is_active {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            };

            let num = if i < 9 {
                format!("{} ", i + 1)
            } else {
                "  ".to_string()
            };

            ListItem::new(Line::from(vec![
                Span::styled(num, Style::default().fg(DIM)),
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
            .border_type(BorderType::Rounded)
            .border_style(border_style(true)),
    );

    f.render_widget(list, area);
}

/// Render the messages area.
fn render_messages(f: &mut Frame, app: &App, area: Rect) {
    let inner_height = area.height.saturating_sub(2) as usize;

    let lines: Vec<Line> = app
        .messages
        .iter()
        .flat_map(|msg| {
            let prefix_style: Style = match msg.role {
                MessageRole::User => Style::default().fg(USER_MSG).add_modifier(Modifier::BOLD),
                MessageRole::Assistant => {
                    Style::default()
                        .fg(ASSISTANT_MSG)
                        .add_modifier(Modifier::BOLD)
                }
                MessageRole::System => {
                    Style::default()
                        .fg(SYSTEM_MSG)
                        .add_modifier(Modifier::ITALIC)
                }
                MessageRole::Tool => {
                    Style::default()
                        .fg(TOOL_MSG)
                        .add_modifier(Modifier::ITALIC)
                }
            };

            let role_color = match msg.role {
                MessageRole::User => USER_MSG,
                MessageRole::Assistant => ASSISTANT_MSG,
                MessageRole::System => SYSTEM_MSG,
                MessageRole::Tool => TOOL_MSG,
            };
            let prefix = match msg.role {
                MessageRole::User => "You",
                MessageRole::Assistant => "Agent",
                MessageRole::System => "System",
                MessageRole::Tool => "Tool",
            };

            let mut result = Vec::new();

            if let Some(ref thinking) = msg.thinking {
                for line in thinking.lines() {
                    result.push(Line::from(Span::styled(
                        format!("  {}", line),
                        Style::default().fg(WARN).italic(),
                    )));
                }
                result.push(Line::from(Span::raw("")));
            }

            let content_lines: Vec<&str> = msg.content.lines().collect();
            for (i, line) in content_lines.iter().enumerate() {
                if i == 0 {
                    result.push(Line::from(vec![
                        Span::styled(
                            format!(" {} ", prefix),
                            Style::default()
                                .fg(Color::Black)
                                .bg(role_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(format!(" {}", line), prefix_style),
                    ]));
                } else {
                    result.push(Line::from(Span::styled(
                        format!("   {}", line),
                        Style::default().fg(TEXT),
                    )));
                }
            }
            result.push(Line::from(Span::raw("")));
            result
        })
        .collect();

    let total_lines = lines.len();
    let scroll = if app.scroll_offset == usize::MAX {
        total_lines.saturating_sub(inner_height)
    } else {
        app.scroll_offset
            .min(total_lines.saturating_sub(inner_height))
    };

    let active_session = app.active_session();
    let chat_title = match active_session {
        Some(s) => format!(" {} ", s.name),
        None => " Chat ".to_string(),
    };

    let paragraph = Paragraph::new(lines)
        .scroll((scroll as u16, 0))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .title(chat_title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(border_style(true)),
        );

    f.render_widget(paragraph, area);
}

/// Render the input area.
fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let mode_tag = match app.agent_mode {
        AgentMode::Plan => "Plan",
        AgentMode::Act => "Act",
        AgentMode::Auto => "Auto",
    };

    let is_disabled = app.agent_busy;
    let (title, border_color) = match app.input_mode {
        InputMode::Normal => {
            if is_disabled {
                (
                    format!(" Input [{}] (waiting...) ", mode_tag),
                    DIM,
                )
            } else {
                (
                    format!(" Input [{}] ", mode_tag),
                    ACCENT,
                )
            }
        }
        InputMode::Command => (" Command ".to_string(), WARN),
    };

    let command_text = format!("/{}", app.command_input);
    let input_text = match app.input_mode {
        InputMode::Normal => app.input.as_str(),
        InputMode::Command => command_text.as_str(),
    };

    let inner_x = area.x + 1;
    let inner_y = area.y + 1;
    let cursor_byte = match app.input_mode {
        InputMode::Normal => app.input_cursor,
        InputMode::Command => command_text.len(),
    };
    let cursor_char = match app.input_mode {
        InputMode::Normal => app.input[..cursor_byte].chars().count(),
        InputMode::Command => command_text[..cursor_byte].chars().count(),
    };

    let input = Paragraph::new(input_text)
        .style(if is_disabled {
            Style::default().fg(DIM)
        } else {
            Style::default().fg(TEXT)
        })
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color)),
        );

    f.render_widget(input, area);

    if area.width > 2 && area.height > 2 && !app.agent_busy {
        let max_x = area.width.saturating_sub(2) as usize;
        let cursor_x = (inner_x as usize + cursor_char).min(inner_x as usize + max_x);
        f.set_cursor_position((cursor_x as u16, inner_y));
    }
}

/// Render the status bar at the bottom.
fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let session_count = app.sessions.total_count();
    let help = match app.input_mode {
        InputMode::Normal => {
            let mode_hint = match app.agent_mode {
                AgentMode::Plan => "Ctrl+P:act",
                AgentMode::Act => "Ctrl+P:plan",
                AgentMode::Auto => "Ctrl+P:plan",
            };
            let base = format!(" Enter:send | /:cmd | {}", mode_hint);
            if session_count > 1 {
                format!("{} | Ctrl+1-9:switch | Ctrl+N:new", base)
            } else {
                format!("{} | Ctrl+N:new", base)
            }
        }
        InputMode::Command => " Enter:exec | Esc:cancel | Tab:autocomplete".to_string(),
    };

    let status_text = if app.agent_busy {
        let spinner_chars = ['⟳', '⟳', '⟳', '⟳'];
        let spinner = spinner_chars[app.spinner_frame as usize % 4];
        if app.busy_message.is_empty() {
            format!(" {} Processing...    {}", spinner, help)
        } else {
            format!(" {} {}    {}", spinner, app.busy_message, help)
        }
    } else if app.status.is_empty() {
        help
    } else {
        format!(" {}    {}", app.status, help)
    };

    let bg = if app.agent_busy {
        Color::Rgb(20, 40, 60)
    } else {
        Color::Black
    };

    let fg_color = if app.agent_busy {
        ACCENT
    } else {
        DIM
    };

    let paragraph = Paragraph::new(status_text)
        .style(Style::default().fg(fg_color).bg(bg));

    f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_render_doesnt_panic() {
        let app = App::new(PathBuf::from("/tmp"));
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
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
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn test_render_in_command_mode() {
        let mut app = App::new(PathBuf::from("/tmp"));
        app.create_session("test");
        app.enter_command_mode();
        app.command_input = "he".to_string();
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn test_command_suggestions_filtering() {
        assert!(!COMMANDS.is_empty());
        assert!(COMMANDS.iter().any(|(cmd, _)| cmd.starts_with("new")));
        assert!(COMMANDS.iter().any(|(cmd, _)| cmd.starts_with("quit")));
    }
}
