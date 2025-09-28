use crate::FjallFilter;

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
    pub filter: FjallFilter,
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
        if self.current_view_level < self.filter.config().max_levels {
            match self.filter.storage.levels.get(self.current_view_level) {
                Some(level) => level.iter().map(|b| *b).collect(),
                None => vec![false; self.filter.config().capacity],
            }
        } else {
            vec![false; self.filter.config().capacity]
        }
    }
}
