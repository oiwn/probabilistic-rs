use crate::RedbSlidingBloomFilter;
use ratatui::crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::{io, path::PathBuf, time::Duration};

// Add this function to handle the TUI
fn run_tui(db_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let config = create_filter_config();
    let filter = RedbSlidingBloomFilter::new(config, db_path.clone())?;
    let app = App {
        filter,
        input: String::new(),
        messages: vec![
            format!("Bloom Filter TUI - Database: {}", db_path.display()),
            "Press 'i' to insert, 'c' to check, 'e' to clean expired, 'q' to quit".to_string(),
        ],
        input_mode: InputMode::Normal,
    };

    // Run the app
    let res = run_app(&mut terminal, app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}

enum InputMode {
    Normal,
    Inserting,
    Checking,
}

struct App {
    filter: RedbSlidingBloomFilter,
    input: String,
    messages: Vec<String>,
    input_mode: InputMode,
}

fn run_app<B: Backend>(
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
                            app.messages.push("Enter element to insert (press Enter when done):".to_string());
                        }
                        KeyCode::Char('c') => {
                            app.input_mode = InputMode::Checking;
                            app.input.clear();
                            app.messages.push(
                                "Enter element to check (press Enter when done):"
                                    .to_string(),
                            );
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
                                                    app.messages.push(format!("'{}' exists in the filter", input));
                                                } else {
                                                    app.messages.push(format!("'{}' does not exist in the filter", input));
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

fn ui<B: Backend>(f: &mut Frame<B>, app: &App) {
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
        .split(f.size());

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
    let mut text = Text::from(Spans::from(msg));
    text.patch_style(style);
    let help_message =
        Paragraph::new(text).style(Style::default().fg(Color::Cyan));
    f.render_widget(help_message, chunks[0]);

    // Input
    let input = Paragraph::new(app.input.as_ref())
        .style(match app.input_mode {
            InputMode::Normal => Style::default(),
            _ => Style::default().fg(Color::Yellow),
        })
        .block(Block::default().borders(Borders::ALL).title("Input"));
    f.render_widget(input, chunks[1]);

    // Set cursor position
    match app.input_mode {
        InputMode::Normal => {}
        _ => f.set_cursor(
            chunks[1].x + app.input.width() as u16 + 1,
            chunks[1].y + 1,
        ),
    }

    // Messages
    let messages: Vec<ListItem> = app
        .messages
        .iter()
        .map(|m| ListItem::new(Spans::from(Span::raw(m))))
        .collect();
    let messages = List::new(messages)
        .block(Block::default().borders(Borders::ALL).title("Messages"))
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol(">> ");
    f.render_widget(messages, chunks[2]);
}
