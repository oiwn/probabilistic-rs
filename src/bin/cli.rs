use clap::{Parser, Subcommand};
use expiring_bloom_rs::{
    ExpiringBloomFilter, FilterConfigBuilder, RedbFilter,
    RedbFilterConfigBuilder, optimal_bit_vector_size, optimal_num_hashes,
    tui::{App, AppMessage, InputMode, MessageType, run_app},
};
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
    io,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new Bloom filter database with custom configuration
    Create {
        /// Path to the database file
        #[arg(short, long)]
        db_path: PathBuf,

        /// Filter capacity
        #[arg(short, long, default_value = "10000")]
        capacity: usize,

        /// False positive rate (between 0 and 1)
        #[arg(short, long, default_value = "0.01")]
        fpr: f64,

        /// Number of levels
        #[arg(short, long, default_value = "5")]
        levels: usize,

        /// Level duration in seconds
        #[arg(long, default_value = "60")]
        duration: u64,
    },

    /// Load a Bloom filter database and perform operations
    Load {
        /// Path to the database file
        #[arg(short, long)]
        db_path: PathBuf,

        #[command(subcommand)]
        operation: LoadCommands,
    },

    /// Start the TUI interface
    Tui {
        /// Path to the database file
        #[arg(short, long)]
        db_path: PathBuf,
    },
}

#[derive(Subcommand)]
enum LoadCommands {
    /// Insert an element into the Bloom filter
    Insert {
        /// Element to insert
        #[arg(short, long)]
        element: String,
    },

    /// Check if an element exists in the Bloom filter
    Check {
        /// Element to check
        #[arg(short, long)]
        element: String,
    },

    /// Cleanup the entire database (with confirmation)
    Cleanup {
        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Display information about the filter
    Info,

    /// Clean expired levels
    CleanExpired,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Create {
            db_path,
            capacity,
            fpr,
            levels,
            duration,
        } => {
            if db_path.exists() {
                println!(
                    "Error: Database already exists at {}",
                    db_path.display()
                );
                println!(
                    "Use the 'load' command to operate on existing databases."
                );
                return Ok(());
            }

            let config = FilterConfigBuilder::default()
                .capacity(*capacity)
                .false_positive_rate(*fpr)
                .level_duration(Duration::from_secs(*duration))
                .max_levels(*levels)
                .build()
                .expect("Failed to build filter config");

            // Create the RedbFilterConfig
            let redb_config = RedbFilterConfigBuilder::default()
                .db_path(db_path.clone())
                .filter_config(Some(config))
                .snapshot_interval(Duration::from_secs(60))
                .build()
                .expect("Failed to build RedbFilterConfig");

            let _filter = RedbFilter::new(redb_config)?;

            println!(
                "Created new Bloom filter database at {}",
                db_path.display()
            );
            println!("Configuration:");
            println!("  Capacity: {capacity}");
            println!("  False positive rate: {fpr}");
            println!("  Levels: {levels}");
            println!("  Duration: {duration} seconds");
        }
        Commands::Load { db_path, operation } => {
            println!("Load command");
            handle_load_command(db_path.to_path_buf(), operation)?;
        }
        Commands::Tui { db_path } => {
            println!("Run tui: {}", db_path.as_path().to_str().unwrap());
            run_tui(db_path)?;
        }
    }

    Ok(())
}

// Add to src/bin/cli.rs
fn handle_load_command(
    db_path: PathBuf,
    operation: &LoadCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create RedbFilterConfig for an existing database
    let redb_config = RedbFilterConfigBuilder::default()
        .db_path(db_path.clone())
        .snapshot_interval(Duration::from_secs(60))
        .build()
        .expect("Failed to build RedbFilterConfig");

    // Create the filter
    let mut filter = RedbFilter::new(redb_config)?;

    match operation {
        LoadCommands::Insert { element } => {
            filter.insert(element.as_bytes())?;
            println!("Element '{element}' inserted successfully");
        }
        LoadCommands::Check { element } => {
            let exists = filter.query(element.as_bytes())?;
            if exists {
                println!("Element '{element}' exists in the filter");
            } else {
                println!("Element '{element}' does not exist in the filter");
            }
        }
        LoadCommands::Info => {
            let filter_config = filter.config();
            println!("Bloom Filter Configuration:");
            println!("  Database path: {}", db_path.display());
            println!("  Capacity: {}", filter_config.capacity);
            println!(
                "  False positive rate: {:.4}",
                filter_config.false_positive_rate
            );
            println!("  Max levels: {}", filter_config.max_levels);
            println!("  Level duration: {:?}", filter_config.level_duration);

            // Calculate additional stats
            let bit_vector_size = optimal_bit_vector_size(
                filter_config.capacity,
                filter_config.false_positive_rate,
            );
            let num_hashes =
                optimal_num_hashes(filter_config.capacity, bit_vector_size);

            println!("  Bit vector size: {bit_vector_size}");
            println!("  Number of hash functions: {num_hashes}");

            // Try to estimate current usage
            println!("\nCurrent State:");
            println!("  Current level index: {}", filter.current_level_index());
        }
        LoadCommands::Cleanup { force } => {
            if *force
                || confirm_action(
                    "Are you sure you want to cleanup the entire database?",
                )
            {
                filter.cleanup_expired_levels()?;
                println!("Database cleaned up successfully");
            } else {
                println!("Cleanup cancelled");
            }
        }
        LoadCommands::CleanExpired => {
            filter.cleanup_expired_levels()?;
            println!("Expired levels cleaned up successfully");
        }
    }

    Ok(())
}

fn confirm_action(prompt: &str) -> bool {
    use std::io::{self, Write};

    print!("{prompt} [y/N]: ");
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    input.trim().to_lowercase() == "y"
}

// Add this function to handle the TUI
pub fn run_tui(db_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create the RedbFilterConfig
    let redb_config = RedbFilterConfigBuilder::default()
        .db_path(db_path.to_path_buf())
        .snapshot_interval(Duration::from_secs(60))
        .build()
        .expect("Failed to build RedbFilterConfig");

    // Create app state
    let filter = RedbFilter::new(redb_config)?;

    let app = App {
        filter,
        input: String::new(),
        messages: vec![AppMessage {
            content: format!(
                "Bloom Filter TUI - Database: {}",
                db_path.display()
            ),
            msg_type: MessageType::Info,
        }],
        input_mode: InputMode::Normal,
        current_view_level: 0, // Start at level 0
        view_offset: 0,        // Start at beginning of bit array
        bits_per_row: 64,      // Default 64 bits per row
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
        println!("{err:?}");
    }

    Ok(())
}
