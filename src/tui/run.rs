use super::{App, AppMessage, InputMode, MessageType, ui};
use crate::ExpiringBloomFilter;
use ratatui::crossterm::event::{self, Event, KeyCode};
use ratatui::{Terminal, backend::Backend};
use std::{io, time::Duration};

pub fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
) -> io::Result<()> {
    // Initialize app with sensible defaults
    app.current_view_level = app.filter.current_level_index();
    app.view_offset = 0;
    app.bits_per_row = 64; // Show 64 bits per row by default

    loop {
        terminal.draw(|f| ui(f, &app))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            match app.input_mode {
                InputMode::Normal => match key.code {
                    KeyCode::Char('i') => {
                        app.input_mode = InputMode::Inserting;
                        app.input.clear();
                    }
                    KeyCode::Char('c') => {
                        app.input_mode = InputMode::Checking;
                        app.input.clear();
                    }
                    KeyCode::Char('e') => {
                        if let Err(e) = app.filter.cleanup_expired_levels() {
                            let content =
                                format!("Error cleaning expired levels: {e}");
                            app.messages.push(AppMessage {
                                content,
                                msg_type: MessageType::Error,
                            });
                        } else {
                            app.messages.push(AppMessage {
                                content: "Cleaned up expired levels".to_string(),
                                msg_type: MessageType::Success,
                            });
                        }
                    }
                    KeyCode::Char('q') => {
                        return Ok(());
                    }
                    // New controls for bit visualization
                    KeyCode::Right => {
                        // Scroll right in bit view
                        app.view_offset =
                            app.view_offset.saturating_add(app.bits_per_row);
                        let max_offset =
                            app.filter.config().capacity.saturating_sub(1);
                        if app.view_offset > max_offset {
                            app.view_offset = max_offset;
                        }
                    }
                    KeyCode::Left => {
                        // Scroll left in bit view
                        app.view_offset =
                            app.view_offset.saturating_sub(app.bits_per_row);
                    }
                    KeyCode::Down => {
                        // Next level
                        app.current_view_level = (app.current_view_level + 1)
                            % app.filter.config().max_levels;
                        app.messages.push(AppMessage {
                            content: format!(
                                "Viewing level {}",
                                app.current_view_level
                            ),
                            msg_type: MessageType::Info,
                        });
                    }
                    KeyCode::Up => {
                        // Previous level
                        if app.current_view_level > 0 {
                            app.current_view_level -= 1;
                        } else {
                            app.current_view_level =
                                app.filter.config().max_levels - 1;
                        }
                        app.messages.push(AppMessage {
                            content: format!(
                                "Viewing level {}",
                                app.current_view_level
                            ),
                            msg_type: MessageType::Info,
                        });
                    }
                    KeyCode::Char('+') => {
                        // Increase bits per row
                        app.bits_per_row = app.bits_per_row.saturating_add(8);
                        if app.bits_per_row > 128 {
                            app.bits_per_row = 128;
                        }
                    }
                    KeyCode::Char('-') => {
                        // Decrease bits per row
                        app.bits_per_row = app.bits_per_row.saturating_sub(8);
                        if app.bits_per_row < 16 {
                            app.bits_per_row = 16;
                        }
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
                                            "Error inserting element: {e}"
                                        );
                                        app.messages.push(AppMessage {
                                            content,
                                            msg_type: MessageType::Error,
                                        });
                                    } else {
                                        let content =
                                            format!("Inserted: {input}");
                                        app.messages.push(AppMessage {
                                            content,
                                            msg_type: MessageType::Info,
                                        });
                                        // Update to current level after insertion
                                        app.current_view_level =
                                            app.filter.current_level_index();
                                    }
                                }
                                InputMode::Checking => {
                                    match app.filter.query(input.as_bytes()) {
                                        Ok(exists) => {
                                            if exists {
                                                let content =
                                                    format!("'{input}' exists",);
                                                app.messages.push(AppMessage {
                                                    content,
                                                    msg_type:
                                                        MessageType::Success,
                                                });
                                            } else {
                                                let content = format!(
                                                    "'{input}' does not exist",
                                                );
                                                app.messages.push(AppMessage {
                                                    content,
                                                    msg_type: MessageType::Error,
                                                });
                                            }
                                        }
                                        Err(e) => {
                                            let content = format!(
                                                "Error checking element: {e}"
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
