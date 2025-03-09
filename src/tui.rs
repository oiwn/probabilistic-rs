use crate::{RedbSlidingBloomFilter, SlidingBloomFilter};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    crossterm::event::{self, Event, KeyCode},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use std::{io, time::Duration};
use unicode_width::UnicodeWidthStr;

pub enum InputMode {
    Normal,
    Inserting,
    Checking,
}

pub enum MessageType {
    Success, // For "exists" messages - green
    Error,   // For "does not exist" messages - red
    Info,    // For regular informational messages - white
}

pub struct AppMessage {
    pub content: String,
    pub msg_type: MessageType,
}

pub struct App {
    pub filter: RedbSlidingBloomFilter,
    pub input: String,
    pub messages: Vec<AppMessage>,
    pub input_mode: InputMode,
}

pub fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match app.input_mode {
                    InputMode::Normal => match key.code {
                        KeyCode::Char('i') => {
                            app.input_mode = InputMode::Inserting;
                            app.input.clear();
                            // app.messages.push("Enter element to insert (press Enter when done):".to_string());
                        }
                        KeyCode::Char('c') => {
                            app.input_mode = InputMode::Checking;
                            app.input.clear();
                            // app.messages.push(
                            //     "Enter element to check (press Enter when done):"
                            //         .to_string(),
                            // );
                        }
                        KeyCode::Char('e') => {
                            if let Err(e) = app.filter.cleanup_expired_levels() {
                                let content = format!(
                                    "Error cleaning expired levels: {}",
                                    e
                                );
                                app.messages.push(AppMessage {
                                    content,
                                    msg_type: MessageType::Error,
                                });
                            } else {
                                app.messages.push(AppMessage {
                                    content: "Cleaned up expired levels"
                                        .to_string(),
                                    msg_type: MessageType::Success,
                                });
                            }
                        }
                        KeyCode::Char('q') => {
                            return Ok(());
                        }
                        _ => {}
                    },
                    InputMode::Inserting | InputMode::Checking => {
                        match key.code {
                            KeyCode::Enter => {
                                let input = app.input.clone();
                                match app.input_mode {
                                    InputMode::Inserting => {
                                        if let Err(e) =
                                            app.filter.insert(input.as_bytes())
                                        {
                                            let content = format!(
                                                "Error inserting element: {}",
                                                e
                                            );
                                            app.messages.push(AppMessage {
                                                content,
                                                msg_type: MessageType::Error,
                                            });
                                        } else {
                                            let content =
                                                format!("Inserted: {}", input);
                                            app.messages.push(AppMessage {
                                                content,
                                                msg_type: MessageType::Info,
                                            });
                                        }
                                    }
                                    InputMode::Checking => {
                                        match app.filter.query(input.as_bytes()) {
                                            Ok(exists) => {
                                                if exists {
                                                    let content = format!(
                                                        "'{}' exists",
                                                        input
                                                    );
                                                    app.messages
                                                        .push(AppMessage {
                                                        content,
                                                        msg_type:
                                                            MessageType::Success,
                                                    });
                                                } else {
                                                    let content = format!(
                                                        "'{}' does not exist",
                                                        input
                                                    );
                                                    app.messages.push(AppMessage { content, msg_type: MessageType::Error });
                                                }
                                            }
                                            Err(e) => {
                                                let content = format!(
                                                    "Error checking element: {}",
                                                    e
                                                );
                                                app.messages.push(AppMessage {
                                                    content,
                                                    msg_type: MessageType::Error,
                                                });
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                                app.input.clear();
                                app.input_mode = InputMode::Normal;
                            }
                            KeyCode::Char(c) => {
                                app.input.push(c);
                            }
                            KeyCode::Backspace => {
                                app.input.pop();
                            }
                            KeyCode::Esc => {
                                app.input.clear();
                                app.input_mode = InputMode::Normal;
                                app.messages.push(AppMessage {
                                    content: "Cancelled operation".to_string(),
                                    msg_type: MessageType::Info,
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints(
            [
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Min(1),
            ]
            .as_ref(),
        )
        .split(f.area());

    let (msg, style) = match app.input_mode {
        InputMode::Normal => (
            vec![
                Span::raw("Press "),
                Span::styled("i", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" to insert, "),
                Span::styled("c", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" to check, "),
                Span::styled("e", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" to clean expired, "),
                Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" to quit"),
            ],
            Style::default().add_modifier(Modifier::RAPID_BLINK),
        ),
        InputMode::Inserting => (
            vec![
                Span::raw("Inserting mode - press "),
                Span::styled(
                    "Esc",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(" to cancel, "),
                Span::styled(
                    "Enter",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(" to confirm"),
            ],
            Style::default(),
        ),
        InputMode::Checking => (
            vec![
                Span::raw("Checking mode - press "),
                Span::styled(
                    "Esc",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(" to cancel, "),
                Span::styled(
                    "Enter",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(" to confirm"),
            ],
            Style::default(),
        ),
    };

    // TODO: wtf is it
    let text = Text::from(Line::from(msg));
    let text = text.clone().patch_style(style);
    let help_message =
        Paragraph::new(text).style(Style::default().fg(Color::Cyan));
    f.render_widget(help_message, chunks[0]);

    // Input
    let input = Paragraph::new(app.input.as_str())
        .style(match app.input_mode {
            InputMode::Normal => Style::default(),
            _ => Style::default().fg(Color::Yellow),
        })
        .block(Block::default().borders(Borders::ALL).title("Input"));
    f.render_widget(input, chunks[1]);

    // Set cursor position
    match app.input_mode {
        InputMode::Normal => {}
        _ => f.set_cursor_position(
            // Use correct width calculation for Unicode strings
            (chunks[1].x + app.input.width() as u16 + 1, chunks[1].y + 1),
        ),
    }

    // Rendering messages
    let messages: Vec<ListItem> = app
        .messages
        .iter()
        .map(|m| {
            let style = match m.msg_type {
                MessageType::Success => Style::default().fg(Color::Green),
                MessageType::Error => Style::default().fg(Color::Red),
                MessageType::Info => Style::default().fg(Color::White),
            };
            ListItem::new(Line::from(Span::styled(&m.content, style)))
        })
        .collect();

    let messages = List::new(messages)
        .block(Block::default().borders(Borders::ALL).title("Messages"))
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol(">> ");
    f.render_widget(messages, chunks[2]);
}
