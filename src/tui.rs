use crate::{RedbFilter, SlidingBloomFilter};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    crossterm::event::{self, Event, KeyCode},
    layout::{Constraint, Direction, Layout, Rect},
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
    pub filter: RedbFilter,
    pub input: String,
    pub messages: Vec<AppMessage>,
    pub input_mode: InputMode,
    pub current_view_level: usize, // Track which level we're viewing
    pub view_offset: usize,        // For scrolling through large bit arrays
    pub bits_per_row: usize,       // How many bits to show per row
}

impl App {
    // Helper method to get bits from the current view level
    pub fn get_current_level_bits(&self) -> Vec<bool> {
        // This is a safe approach to get the bits from the current level
        if self.current_view_level < self.filter.get_config().max_levels {
            match self.filter.storage.levels.get(self.current_view_level) {
                Some(level) => level.clone(),
                None => vec![false; self.filter.get_config().capacity],
            }
        } else {
            vec![false; self.filter.get_config().capacity]
        }
    }
}

pub fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
) -> io::Result<()> {
    // Initialize app with sensible defaults
    app.current_view_level = app.filter.get_current_level_index();
    app.view_offset = 0;
    app.bits_per_row = 64; // Show 64 bits per row by default

    loop {
        terminal.draw(|f| ui(f, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
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
                        // New controls for bit visualization
                        KeyCode::Right => {
                            // Scroll right in bit view
                            app.view_offset =
                                app.view_offset.saturating_add(app.bits_per_row);
                            let max_offset = app
                                .filter
                                .get_config()
                                .capacity
                                .saturating_sub(1);
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
                                % app.filter.get_config().max_levels;
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
                                    app.filter.get_config().max_levels - 1;
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
                                            // Update to current level after insertion
                                            app.current_view_level = app
                                                .filter
                                                .get_current_level_index();
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
    // Create a layout with a main horizontal split
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [Constraint::Percentage(30), Constraint::Percentage(70)].as_ref(),
        )
        .margin(1)
        .split(f.area());

    // Left side for bit visualization
    let bit_viz_area = main_chunks[0];
    render_bit_visualization(f, app, bit_viz_area);

    // Right side for controls and messages
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Min(1),
            ]
            .as_ref(),
        )
        .split(main_chunks[1]);

    let (msg, style) = match app.input_mode {
        InputMode::Normal => (
            vec![
                Span::raw("Press "),
                Span::styled("i", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" to insert, "),
                Span::styled("c", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" to check, "),
                Span::styled("e", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" to clean exp., "),
                Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" to quit"),
            ],
            Style::default().add_modifier(Modifier::RAPID_BLINK),
        ),
        InputMode::Inserting => (
            vec![
                Span::raw("Inserting - press "),
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
                Span::raw("Checking - press "),
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

    let text = Text::from(Line::from(msg));
    let text = text.clone().patch_style(style);
    let help_message =
        Paragraph::new(text).style(Style::default().fg(Color::Cyan));
    f.render_widget(help_message, right_chunks[0]);

    // Input
    let input = Paragraph::new(app.input.as_str())
        .style(match app.input_mode {
            InputMode::Normal => Style::default(),
            _ => Style::default().fg(Color::Yellow),
        })
        .block(Block::default().borders(Borders::ALL).title("Input"));
    f.render_widget(input, right_chunks[1]);

    // Set cursor position
    match app.input_mode {
        InputMode::Normal => {}
        _ => f.set_cursor_position(
            // Use correct width calculation for Unicode strings
            (
                right_chunks[1].x + app.input.width() as u16 + 1,
                right_chunks[1].y + 1,
            ),
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
    f.render_widget(messages, right_chunks[2]);
}

fn render_bit_visualization(f: &mut Frame, app: &App, area: Rect) {
    // Create a block for the bit visualization
    let block = Block::default().borders(Borders::ALL).title(format!(
        "Filter Bits - Level {} (↑↓ to change)",
        app.current_view_level
    ));

    // Render the block
    let inner_area = block.inner(area);
    f.render_widget(block, area);

    // Get the bits for the current level
    let bits = app.get_current_level_bits();

    // Calculate visible range based on offset and area size
    let start_offset = app.view_offset;
    let end_offset = std::cmp::min(
        start_offset + (inner_area.height as usize * app.bits_per_row),
        bits.len(),
    );

    // Track position for rendering bits
    let mut y_pos = 0;
    let mut x_pos;

    // Render the bits
    for chunk_start in (start_offset..end_offset).step_by(app.bits_per_row) {
        if y_pos >= inner_area.height {
            break; // No more room to render
        }

        // Create a new line at this y position
        let mut line_spans = Vec::new();

        // Add index marker at the start of line
        line_spans.push(Span::styled(
            format!("{:06} ", chunk_start),
            Style::default().fg(Color::DarkGray),
        ));

        // Add bits in this row
        x_pos = 0;
        #[allow(clippy::needless_range_loop)]
        for i in
            chunk_start..std::cmp::min(chunk_start + app.bits_per_row, bits.len())
        {
            // Group bits for readability (every 8 bits)
            if x_pos > 0 && x_pos % 8 == 0 {
                line_spans.push(Span::raw(" "));
            }

            // Color code: green for set bits (true), red for unset bits (false)
            let bit_color = if bits[i] { Color::Green } else { Color::Red };
            line_spans.push(Span::styled(
                if bits[i] { "1" } else { "0" },
                Style::default().fg(bit_color),
            ));

            x_pos += 1;
        }

        // Render this line of bits
        let text = Text::from(Line::from(line_spans));
        let paragraph = Paragraph::new(text);

        let bit_line_area = Rect {
            x: inner_area.x,
            y: inner_area.y + y_pos,
            width: inner_area.width,
            height: 1,
        };

        f.render_widget(paragraph, bit_line_area);
        y_pos += 1;
    }

    // Add navigation help at the bottom if there's room
    if y_pos < inner_area.height {
        let help_text = Text::from(vec![Line::from(Span::styled(
            "← → to scroll, + - to adjust view",
            Style::default().fg(Color::Yellow),
        ))]);
        let help_area = Rect {
            x: inner_area.x,
            y: inner_area.y + inner_area.height - 1,
            width: inner_area.width,
            height: 1,
        };
        f.render_widget(Paragraph::new(help_text), help_area);
    }
}

/* use crate::{RedbSlidingBloomFilter, SlidingBloomFilter};
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
} */
