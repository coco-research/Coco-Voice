//! In-memory storage for the most recent completed dictation take.
//!
//! Used by the menu bar "Paste last take" and "Copy last" actions when
//! "Never" (`never_store_history`) is enabled so nothing is written to disk
//! and the take clears automatically on quit.

use std::sync::Mutex;

pub struct LastTake(Mutex<Option<String>>);

impl Default for LastTake {
    fn default() -> Self {
        Self::new()
    }
}

impl LastTake {
    pub fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub fn set(&self, text: String) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.0.lock() {
            *guard = Some(trimmed.to_string());
        }
    }

    pub fn get(&self) -> Option<String> {
        self.0.lock().ok().and_then(|g| g.clone())
    }

    #[cfg(test)]
    pub fn clear(&self) {
        if let Ok(mut guard) = self.0.lock() {
            *guard = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LastTake;

    #[test]
    fn last_take_holds_text_in_memory_only_and_starts_empty_after_restart() {
        let mem = LastTake::new();
        assert_eq!(mem.get(), None);

        mem.set("   ".to_string());
        assert_eq!(mem.get(), None);

        mem.set("purple elephant forty two".to_string());
        assert_eq!(mem.get().as_deref(), Some("purple elephant forty two"));

        mem.clear();
        assert_eq!(mem.get(), None);

        // Fresh instance simulates app restart: empty again.
        let restarted = LastTake::new();
        assert_eq!(restarted.get(), None);
    }
}
