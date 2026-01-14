//! Scroll mode management for terminal output.

use crossterm::event::{KeyCode, KeyEvent};

/// Manages scroll state for terminal output.
#[derive(Debug, Clone)]
pub struct ScrollManager {
    /// Current scroll offset (0 = bottom/live output).
    offset: usize,
    /// Total lines available in terminal history.
    total_lines: usize,
    /// Viewport height (visible lines).
    viewport_height: usize,
}

impl ScrollManager {
    /// Creates a new scroll manager.
    pub fn new() -> Self {
        Self {
            offset: 0,
            total_lines: 0,
            viewport_height: 24,
        }
    }

    /// Updates total lines and viewport height.
    pub fn update_dimensions(&mut self, total_lines: usize, viewport_height: usize) {
        self.total_lines = total_lines;
        self.viewport_height = viewport_height;
        self.clamp_offset();
    }

    /// Returns current scroll offset.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Handles navigation key in scroll mode.
    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.scroll_down(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_up(1),
            KeyCode::PageDown => self.scroll_down(self.viewport_height),
            KeyCode::PageUp => self.scroll_up(self.viewport_height),
            KeyCode::Char('g') => self.jump_to_top(),
            KeyCode::Char('G') => self.jump_to_bottom(),
            _ => {}
        }
    }

    /// Scrolls down by n lines.
    fn scroll_down(&mut self, n: usize) {
        self.offset = self.offset.saturating_sub(n);
    }

    /// Scrolls up by n lines.
    fn scroll_up(&mut self, n: usize) {
        self.offset = (self.offset + n).min(self.max_offset());
    }

    /// Jumps to top of history.
    fn jump_to_top(&mut self) {
        self.offset = self.max_offset();
    }

    /// Jumps to bottom (live output).
    fn jump_to_bottom(&mut self) {
        self.offset = 0;
    }

    /// Returns maximum valid offset.
    fn max_offset(&self) -> usize {
        self.total_lines.saturating_sub(self.viewport_height)
    }

    /// Clamps offset to valid range.
    fn clamp_offset(&mut self) {
        self.offset = self.offset.min(self.max_offset());
    }

    /// Resets to live output (bottom).
    pub fn reset(&mut self) {
        self.offset = 0;
    }
}

impl Default for ScrollManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn new_scroll_manager_starts_at_bottom() {
        let sm = ScrollManager::new();
        assert_eq!(sm.offset(), 0);
    }

    #[test]
    fn scroll_up_increases_offset() {
        let mut sm = ScrollManager::new();
        sm.update_dimensions(100, 24);
        sm.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(sm.offset(), 1);
    }

    #[test]
    fn scroll_down_decreases_offset() {
        let mut sm = ScrollManager::new();
        sm.update_dimensions(100, 24);
        sm.offset = 10;
        sm.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(sm.offset(), 9);
    }

    #[test]
    fn scroll_down_stops_at_zero() {
        let mut sm = ScrollManager::new();
        sm.update_dimensions(100, 24);
        sm.offset = 0;
        sm.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(sm.offset(), 0);
    }

    #[test]
    fn scroll_up_stops_at_max() {
        let mut sm = ScrollManager::new();
        sm.update_dimensions(100, 24);
        for _ in 0..200 {
            sm.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        }
        assert_eq!(sm.offset(), 76); // 100 - 24
    }

    #[test]
    fn page_down_scrolls_viewport_height() {
        let mut sm = ScrollManager::new();
        sm.update_dimensions(100, 24);
        sm.offset = 50;
        sm.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(sm.offset(), 26); // 50 - 24
    }

    #[test]
    fn page_up_scrolls_viewport_height() {
        let mut sm = ScrollManager::new();
        sm.update_dimensions(100, 24);
        sm.offset = 10;
        sm.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(sm.offset(), 34); // 10 + 24
    }

    #[test]
    fn g_jumps_to_top() {
        let mut sm = ScrollManager::new();
        sm.update_dimensions(100, 24);
        sm.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(sm.offset(), 76); // max offset
    }

    #[test]
    fn capital_g_jumps_to_bottom() {
        let mut sm = ScrollManager::new();
        sm.update_dimensions(100, 24);
        sm.offset = 50;
        sm.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE));
        assert_eq!(sm.offset(), 0);
    }

    #[test]
    fn reset_returns_to_bottom() {
        let mut sm = ScrollManager::new();
        sm.update_dimensions(100, 24);
        sm.offset = 50;
        sm.reset();
        assert_eq!(sm.offset(), 0);
    }

    #[test]
    fn arrow_keys_work_like_jk() {
        let mut sm = ScrollManager::new();
        sm.update_dimensions(100, 24);
        sm.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(sm.offset(), 1);
        sm.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(sm.offset(), 0);
    }
}
