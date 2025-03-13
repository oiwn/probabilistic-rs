use super::{App, InputMode, MessageType};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use unicode_width::UnicodeWidthStr;

pub fn ui(f: &mut Frame, app: &App) {
    // Create a layout with a main horizontal split
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [Constraint::Percentage(50), Constraint::Percentage(50)].as_ref(),
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
