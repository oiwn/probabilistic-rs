use expiring_bloom_rs::{
    ExpiringBloomFilter, FilterConfigBuilder, InMemoryFilter,
    tui::{App, AppMessage, InputMode, MessageType},
};
use rand::{Rng, seq::IndexedRandom};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{DisableMouseCapture, EnableMouseCapture},
        execute,
        terminal::{
            EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
            enable_raw_mode,
        },
    },
};
use std::{
    collections::{HashMap, HashSet},
    io,
    time::{Duration, Instant},
};

// Constants for the example
const CAPACITY: usize = 1000;
const FALSE_POSITIVE_RATE: f64 = 0.01;
const LEVEL_DURATION_SECS: u64 = 5; // Short duration for demonstration
const MAX_LEVELS: usize = 3;
const DEMO_ITEMS: usize = 200; // Number of items to insert during demo

// Word list for generating readable test items
const WORD_LIST: [&str; 20] = [
    "apple",
    "banana",
    "cherry",
    "date",
    "elderberry",
    "fig",
    "grape",
    "honeydew",
    "kiwi",
    "lemon",
    "mango",
    "nectarine",
    "orange",
    "peach",
    "quince",
    "raspberry",
    "strawberry",
    "tangerine",
    "watermelon",
    "zucchini",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create filter configuration
    let config = FilterConfigBuilder::default()
        .capacity(CAPACITY)
        .false_positive_rate(FALSE_POSITIVE_RATE)
        .level_duration(Duration::from_secs(LEVEL_DURATION_SECS))
        .max_levels(MAX_LEVELS)
        .build()
        .expect("Failed to create filter config");

    println!("Time-Decaying Bloom Filter Workflow Example");
    println!("-------------------------------------------");
    println!("Configuration:");
    println!("• Capacity: {} elements", CAPACITY);
    println!("• False Positive Rate: {:.2}%", FALSE_POSITIVE_RATE * 100.0);
    println!("• Level Duration: {} seconds", LEVEL_DURATION_SECS);
    println!("• Max Levels: {}", MAX_LEVELS);
    println!();
    println!("This example will:");
    println!("1. Create an in-memory time-decaying Bloom filter");
    println!("2. Launch an interactive TUI showing filter bits");
    println!("3. Automatically insert test items at intervals");
    println!("4. Allow you to query items and observe behavior");
    println!("5. Display statistics after exiting the TUI");
    println!();
    println!("Press Enter to continue...");

    let _ = io::stdin().read_line(&mut String::new())?;

    // Create tracking sets for statistics
    let mut inserted_items = HashSet::new();
    let mut query_history = HashMap::new();
    let mut total_true_positives = 0;
    let mut total_true_negatives = 0;
    let mut total_false_positives = 0;
    let mut level_rotations = 0;

    // Create the filter
    let mut filter = InMemoryFilter::new(config.clone())?;

    // Setup for automatic data insertion
    let auto_insert_delay = Duration::from_millis(200); // Insert every 500ms
    let mut next_auto_insert = Instant::now() + auto_insert_delay;
    let mut auto_insert_count = 0;
    let mut demo_items = generate_test_data(DEMO_ITEMS);

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create the app state
    let app = App {
        filter: expiring_bloom_rs::RedbFilter::new(
            expiring_bloom_rs::RedbFilterConfigBuilder::default()
                .db_path("workflow_temp.redb".into())
                .filter_config(Some(config.clone()))
                .build()?,
        )?,
        input: String::new(),
        messages: vec![
            AppMessage {
                content: "Time-Decaying Bloom Filter Workflow".to_string(),
                msg_type: MessageType::Info,
            },
            AppMessage {
                content: format!(
                    "Auto-inserting {} items at {} ms intervals",
                    DEMO_ITEMS,
                    auto_insert_delay.as_millis()
                ),
                msg_type: MessageType::Info,
            },
            AppMessage {
                content: "Press 'c' to check if an item exists".to_string(),
                msg_type: MessageType::Success,
            },
            AppMessage {
                content: "Press 'q' to quit and see statistics".to_string(),
                msg_type: MessageType::Error,
            },
        ],
        input_mode: InputMode::Normal,
        current_view_level: 0,
        view_offset: 0,
        bits_per_row: 64,
    };

    // Custom run loop that integrates auto-insertion
    let result = custom_run_app(
        &mut terminal,
        app,
        &mut filter,
        &mut demo_items,
        &mut inserted_items,
        &mut query_history,
        &mut auto_insert_count,
        &mut next_auto_insert,
        &mut level_rotations,
    );

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        println!("Error: {:?}", err);
    }

    // Calculate final statistics
    for (item, result) in query_history.iter() {
        if *result && inserted_items.contains(item) {
            total_true_positives += 1;
        } else if *result && !inserted_items.contains(item) {
            total_false_positives += 1;
        } else if !*result && !inserted_items.contains(item) {
            total_true_negatives += 1;
        }
        // We don't track false negatives because they shouldn't happen with Bloom filters
    }

    // Print final statistics
    print_statistics(
        &inserted_items,
        total_true_positives,
        total_true_negatives,
        total_false_positives,
        level_rotations,
        &query_history,
        &filter,
    );

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn custom_run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
    filter: &mut InMemoryFilter,
    demo_items: &mut [String],
    inserted_items: &mut HashSet<String>,
    query_history: &mut HashMap<String, bool>,
    auto_insert_count: &mut usize,
    next_auto_insert: &mut Instant,
    level_rotations: &mut usize,
) -> io::Result<()> {
    let _start_time = Instant::now();
    let mut last_level_index = filter.current_level_index;

    loop {
        // Render the TUI
        terminal.draw(|f| expiring_bloom_rs::tui::ui(f, &app))?;

        // Check if it's time for auto-insertion
        let now = Instant::now();
        if now >= *next_auto_insert && *auto_insert_count < demo_items.len() {
            // Get the next item
            let item = &demo_items[*auto_insert_count];

            // Insert into our filter
            if let Err(e) = filter.insert(item.as_bytes()) {
                app.messages.push(AppMessage {
                    content: format!("Error inserting '{}': {}", item, e),
                    msg_type: MessageType::Error,
                });
            } else {
                app.messages.push(AppMessage {
                    content: format!("Auto-inserted: {}", item),
                    msg_type: MessageType::Info,
                });

                // Add to our tracking set
                inserted_items.insert(item.clone());

                // Check if we've rotated to a new level
                if filter.current_level_index != last_level_index {
                    *level_rotations += 1;
                    last_level_index = filter.current_level_index;
                    app.messages.push(AppMessage {
                        content: format!(
                            "Level rotation! Now at level {}",
                            last_level_index
                        ),
                        msg_type: MessageType::Success,
                    });
                }

                // Also insert into the display filter
                if let Some(redb_filter) = app
                    .filter
                    .storage
                    .levels
                    .get_mut(filter.current_level_index)
                {
                    // Copy the bits from our in-memory filter to the display filter
                    for (i, bit) in filter.storage.levels
                        [filter.current_level_index]
                        .iter()
                        .enumerate()
                    {
                        if i < redb_filter.len() {
                            redb_filter.set(i, *bit);
                            // redb_filter[i] = *bit;
                        }
                    }
                }
                // Update the current view level to match the active level
                app.current_view_level = filter.current_level_index;
            }

            *auto_insert_count += 1;
            *next_auto_insert = now + Duration::from_millis(500);
        }

        // Run filter maintenance
        if let Err(e) = filter.cleanup_expired_levels() {
            app.messages.push(AppMessage {
                content: format!("Error cleaning expired levels: {}", e),
                msg_type: MessageType::Error,
            });
        }

        // Process keyboard input
        if ratatui::crossterm::event::poll(Duration::from_millis(100))? {
            if let ratatui::crossterm::event::Event::Key(key) =
                ratatui::crossterm::event::read()?
            {
                match app.input_mode {
                    InputMode::Normal => match key.code {
                        ratatui::crossterm::event::KeyCode::Char('c') => {
                            app.input_mode = InputMode::Checking;
                            app.input.clear();
                        }
                        ratatui::crossterm::event::KeyCode::Char('q') => {
                            return Ok(());
                        }
                        // Level navigation
                        ratatui::crossterm::event::KeyCode::Down => {
                            app.current_view_level =
                                (app.current_view_level + 1) % MAX_LEVELS;
                        }
                        ratatui::crossterm::event::KeyCode::Up => {
                            if app.current_view_level > 0 {
                                app.current_view_level -= 1;
                            } else {
                                app.current_view_level = MAX_LEVELS - 1;
                            }
                        }
                        // Bit view navigation
                        ratatui::crossterm::event::KeyCode::Right => {
                            app.view_offset =
                                app.view_offset.saturating_add(app.bits_per_row);
                            if app.view_offset >= CAPACITY {
                                app.view_offset = CAPACITY - 1;
                            }
                        }
                        ratatui::crossterm::event::KeyCode::Left => {
                            app.view_offset =
                                app.view_offset.saturating_sub(app.bits_per_row);
                        }
                        _ => {}
                    },
                    InputMode::Checking => {
                        match key.code {
                            ratatui::crossterm::event::KeyCode::Enter => {
                                let query_item = app.input.clone();

                                // Query the filter
                                match filter.query(query_item.as_bytes()) {
                                    Ok(exists) => {
                                        if exists {
                                            app.messages.push(AppMessage {
                                                content: format!(
                                                    "'{}' exists in filter",
                                                    query_item
                                                ),
                                                msg_type: MessageType::Success,
                                            });
                                        } else {
                                            app.messages.push(AppMessage {
                                            content: format!("'{}' does not exist in filter", query_item),
                                            msg_type: MessageType::Error,
                                        });
                                        }

                                        // Record query result for statistics
                                        query_history
                                            .insert(query_item.clone(), exists);
                                    }
                                    Err(e) => {
                                        app.messages.push(AppMessage {
                                            content: format!(
                                                "Error querying '{}': {}",
                                                query_item, e
                                            ),
                                            msg_type: MessageType::Error,
                                        });
                                    }
                                }

                                app.input.clear();
                                app.input_mode = InputMode::Normal;
                            }
                            ratatui::crossterm::event::KeyCode::Char(c) => {
                                app.input.push(c);
                            }
                            ratatui::crossterm::event::KeyCode::Backspace => {
                                app.input.pop();
                            }
                            ratatui::crossterm::event::KeyCode::Esc => {
                                app.input.clear();
                                app.input_mode = InputMode::Normal;
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

// Generate test data with more memorable patterns
fn generate_test_data(count: usize) -> Vec<String> {
    let mut rng = rand::rng();
    let mut items = Vec::with_capacity(count);

    for _ in 0..count {
        // Select 1-2 random words
        let num_words = rng.random_range(1..=2);
        let mut selected_words = Vec::with_capacity(num_words);

        for _ in 0..num_words {
            selected_words.push(*WORD_LIST.choose(&mut rng).unwrap());
        }

        // Add a small number for uniqueness
        let number = rng.random_range(1..100);
        items.push(format!("{}_{}", selected_words.join("_"), number));
    }

    items
}

// Print final statistics after the demo
fn print_statistics(
    inserted_items: &HashSet<String>,
    true_positives: usize,
    true_negatives: usize,
    false_positives: usize,
    level_rotations: usize,
    query_history: &HashMap<String, bool>,
    filter: &InMemoryFilter,
) {
    println!("╔═══════════════════════════════════════════════════╗");
    println!("║           BLOOM FILTER WORKFLOW RESULTS           ║");
    println!("╚═══════════════════════════════════════════════════╝");
    println!();

    println!("📊 Insertion Statistics:");
    println!("   Items inserted: {}", inserted_items.len());
    println!("   Level rotations: {}", level_rotations);

    // Calculate bit density for each level
    println!("\n📈 Bit Density by Level:");
    for level in 0..filter.config.max_levels {
        let set_bits = filter.storage.levels[level]
            .iter()
            .filter(|bit| **bit)
            .count();
        let density =
            (set_bits as f64 / filter.storage.levels[level].len() as f64) * 100.0;
        println!(
            "   Level {}: {:.2}% ({}/{} bits set)",
            level,
            density,
            set_bits,
            filter.storage.levels[level].len()
        );
    }

    // Query statistics
    println!("\n🔍 Query Statistics:");
    println!("   Total queries: {}", query_history.len());
    println!("   True positives: {}", true_positives);
    println!("   True negatives: {}", true_negatives);
    println!("   False positives: {}", false_positives);

    // Only calculate if we had queries
    if !query_history.is_empty() {
        let measured_fpr = if true_negatives + false_positives > 0 {
            (false_positives as f64) / (true_negatives + false_positives) as f64
        } else {
            0.0
        };

        println!("\n📉 Filter Performance:");
        println!("   Configured FPR: {:.2}%", FALSE_POSITIVE_RATE * 100.0);
        println!("   Measured FPR: {:.2}%", measured_fpr * 100.0);
        println!(
            "   Accuracy: {:.2}%",
            ((true_positives + true_negatives) as f64
                / query_history.len() as f64)
                * 100.0
        );
    }

    println!("\n⏱️ Time Decay Information:");
    println!("   Level duration: {} seconds", LEVEL_DURATION_SECS);
    println!(
        "   Total decay time: {} seconds",
        LEVEL_DURATION_SECS * MAX_LEVELS as u64
    );

    println!("\nBloom filter characteristics:");
    println!("   Filter capacity: {} elements", CAPACITY);
    println!("   Number of hash functions: {}", filter.num_hashes);
    println!(
        "   Bits per element: {:.2}",
        (CAPACITY as f64) / (inserted_items.len() as f64)
    );

    println!("\nThank you for exploring time-decaying Bloom filters!");
}
