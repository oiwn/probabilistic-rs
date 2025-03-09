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

pub struct App {
    pub filter: RedbSlidingBloomFilter,
    pub input: String,
    pub messages: Vec<String>,
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
                                app.messages.push(format!(
                                    "Error cleaning expired levels: {}",
                                    e
                                ));
                            } else {
                                app.messages.push(
                                    "Cleaned up expired levels".to_string(),
                                );
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
                                            app.messages.push(format!(
                                                "Error inserting element: {}",
                                                e
                                            ));
                                        } else {
                                            app.messages.push(format!(
                                                "Inserted: {}",
                                                input
                                            ));
                                        }
                                    }
                                    InputMode::Checking => {
                                        match app.filter.query(input.as_bytes()) {
                                            Ok(exists) => {
                                                if exists {
                                                    app.messages.push(format!(
                                                        "'{}' exists",
                                                        input
                                                    ));
                                                } else {
                                                    app.messages.push(format!(
                                                        "'{}' does not exist",
                                                        input
                                                    ));
                                                }
                                            }
                                            Err(e) => app.messages.push(format!(
                                                "Error checking element: {}",
                                                e
                                            )),
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
                                app.messages
                                    .push("Cancelled operation".to_string());
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

    // Messages
    let messages: Vec<ListItem> = app
        .messages
        .iter()
        .map(|m| ListItem::new(Line::from(Span::raw(m))))
        .collect();
    let messages = List::new(messages)
        .block(Block::default().borders(Borders::ALL).title("Messages"))
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol(">> ");
    f.render_widget(messages, chunks[2]);
}
