//! Command input parsing and line editing.
//!
//! Provides an `InputLine` struct that manages a text buffer with cursor
//! movement, editing operations, and command history. Used by the TUI
//! input prompt to handle user keystrokes.

/// A line editor with cursor movement and command history.
///
/// The buffer is maintained as a `Vec<char>` so that cursor-based
/// operations work correctly with multi-byte characters.
pub struct InputLine {
    buffer: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    history_pos: Option<usize>,
    saved_input: String,
}


impl InputLine {
    /// Create a new empty input line.
    pub fn new() -> Self {
        InputLine {
            buffer: Vec::new(),
            cursor: 0,
            history: Vec::new(),
            history_pos: None,
            saved_input: String::new(),
        }
    }

    /// Insert a character at the cursor position.
    pub fn insert(&mut self, ch: char) {
        self.buffer.insert(self.cursor, ch);
        self.cursor += 1;
    }

    /// Delete the character before the cursor (backspace).
    pub fn delete_back(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.buffer.remove(self.cursor);
        }
    }

    /// Delete the character at the cursor position (forward delete).
    pub fn delete_forward(&mut self) {
        if self.cursor < self.buffer.len() {
            self.buffer.remove(self.cursor);
        }
    }

    /// Move the cursor one position to the left.
    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move the cursor one position to the right.
    pub fn move_right(&mut self) {
        if self.cursor < self.buffer.len() {
            self.cursor += 1;
        }
    }

    /// Move the cursor to the beginning of the line.
    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    /// Move the cursor to the end of the line.
    pub fn move_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    /// Move the cursor one word to the left.
    ///
    /// A word boundary is defined as the transition from a non-alphanumeric
    /// character to an alphanumeric character, scanning leftward.
    pub fn move_word_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        // Skip whitespace/punctuation
        let mut pos = self.cursor - 1;
        while pos > 0 && !self.buffer[pos].is_alphanumeric() {
            pos -= 1;
        }
        // Skip word characters
        while pos > 0 && self.buffer[pos - 1].is_alphanumeric() {
            pos -= 1;
        }
        self.cursor = pos;
    }

    /// Move the cursor one word to the right.
    ///
    /// A word boundary is defined as the transition from an alphanumeric
    /// character to a non-alphanumeric character, scanning rightward.
    pub fn move_word_right(&mut self) {
        let len = self.buffer.len();
        if self.cursor >= len {
            return;
        }
        let mut pos = self.cursor;
        // Skip current word characters
        while pos < len && self.buffer[pos].is_alphanumeric() {
            pos += 1;
        }
        // Skip whitespace/punctuation
        while pos < len && !self.buffer[pos].is_alphanumeric() {
            pos += 1;
        }
        self.cursor = pos;
    }

    /// Delete the word before the cursor (Ctrl-W).
    pub fn delete_word_back(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let end = self.cursor;
        // Skip whitespace
        while self.cursor > 0 && !self.buffer[self.cursor - 1].is_alphanumeric() {
            self.cursor -= 1;
        }
        // Skip word characters
        while self.cursor > 0 && self.buffer[self.cursor - 1].is_alphanumeric() {
            self.cursor -= 1;
        }
        self.buffer.drain(self.cursor..end);
    }

    /// Clear the entire buffer and reset the cursor.
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.history_pos = None;
    }

    /// Return the current buffer contents as a String.
    pub fn text(&self) -> String {
        self.buffer.iter().collect()
    }

    /// Return the current cursor position (character index).
    pub fn cursor_pos(&self) -> usize {
        self.cursor
    }

    /// Return whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Submit the current line: add it to history, clear the buffer,
    /// and return the submitted text.
    pub fn submit(&mut self) -> String {
        let text = self.text();
        if !text.is_empty() {
            self.history.push(text.clone());
        }
        self.clear();
        text
    }

    /// Navigate up through command history.
    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        match self.history_pos {
            None => {
                // Save current input and go to most recent history
                self.saved_input = self.text();
                self.history_pos = Some(self.history.len() - 1);
            }
            Some(pos) => {
                if pos > 0 {
                    self.history_pos = Some(pos - 1);
                } else {
                    return; // Already at oldest entry
                }
            }
        }
        let entry = self.history[self.history_pos.unwrap()].clone();
        self.buffer = entry.chars().collect();
        self.cursor = self.buffer.len();
    }

    /// Navigate down through command history.
    pub fn history_down(&mut self) {
        match self.history_pos {
            None => return, // Not browsing history
            Some(pos) => {
                if pos < self.history.len() - 1 {
                    self.history_pos = Some(pos + 1);
                    let entry = self.history[pos + 1].clone();
                    self.buffer = entry.chars().collect();
                    self.cursor = self.buffer.len();
                } else {
                    // Return to saved input
                    self.history_pos = None;
                    self.buffer = self.saved_input.chars().collect();
                    self.cursor = self.buffer.len();
                }
            }
        }
    }

    /// Render the input line with a prompt, formatted for display.
    ///
    /// Returns a string like "cmx> some input" with the cursor position
    /// indicated (the cursor position is returned as a separate value
    /// through the character offset from the start of the rendered string).
    pub fn render(&self, prompt: &str, width: usize) -> String {
        let text = self.text();
        let prompt_len = prompt.chars().count();
        let available = if width > prompt_len { width - prompt_len } else { 0 };

        let display_text = if text.chars().count() > available {
            // Show the portion around the cursor
            let start = if self.cursor > available {
                self.cursor - available + 1
            } else {
                0
            };
            let chars: Vec<char> = text.chars().collect();
            let end = (start + available).min(chars.len());
            chars[start..end].iter().collect::<String>()
        } else {
            text
        };

        format!("{}{}", prompt, display_text)
    }

    /// Return the number of entries in the command history.
    pub fn history_len(&self) -> usize {
        self.history.len()
    }
}


impl Default for InputLine {
    fn default() -> Self {
        Self::new()
    }
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty() {
        let input = InputLine::new();
        assert!(input.is_empty());
        assert_eq!(input.text(), "");
        assert_eq!(input.cursor_pos(), 0);
    }

    #[test]
    fn insert_characters() {
        let mut input = InputLine::new();
        input.insert('h');
        input.insert('i');
        assert_eq!(input.text(), "hi");
        assert_eq!(input.cursor_pos(), 2);
    }

    #[test]
    fn insert_at_cursor() {
        let mut input = InputLine::new();
        input.insert('a');
        input.insert('c');
        input.move_left();
        input.insert('b');
        assert_eq!(input.text(), "abc");
        assert_eq!(input.cursor_pos(), 2);
    }

    #[test]
    fn delete_back() {
        let mut input = InputLine::new();
        input.insert('a');
        input.insert('b');
        input.insert('c');
        input.delete_back();
        assert_eq!(input.text(), "ab");
        assert_eq!(input.cursor_pos(), 2);
    }

    #[test]
    fn delete_back_at_start() {
        let mut input = InputLine::new();
        input.insert('a');
        input.move_home();
        input.delete_back(); // Should do nothing
        assert_eq!(input.text(), "a");
        assert_eq!(input.cursor_pos(), 0);
    }

    #[test]
    fn delete_back_empty() {
        let mut input = InputLine::new();
        input.delete_back(); // Should do nothing
        assert!(input.is_empty());
    }

    #[test]
    fn delete_forward() {
        let mut input = InputLine::new();
        input.insert('a');
        input.insert('b');
        input.insert('c');
        input.move_home();
        input.delete_forward();
        assert_eq!(input.text(), "bc");
        assert_eq!(input.cursor_pos(), 0);
    }

    #[test]
    fn delete_forward_at_end() {
        let mut input = InputLine::new();
        input.insert('a');
        input.delete_forward(); // Should do nothing
        assert_eq!(input.text(), "a");
    }

    #[test]
    fn move_left_right() {
        let mut input = InputLine::new();
        input.insert('a');
        input.insert('b');
        input.insert('c');
        assert_eq!(input.cursor_pos(), 3);

        input.move_left();
        assert_eq!(input.cursor_pos(), 2);

        input.move_left();
        assert_eq!(input.cursor_pos(), 1);

        input.move_right();
        assert_eq!(input.cursor_pos(), 2);
    }

    #[test]
    fn move_left_at_start() {
        let mut input = InputLine::new();
        input.insert('a');
        input.move_home();
        input.move_left(); // Should stay at 0
        assert_eq!(input.cursor_pos(), 0);
    }

    #[test]
    fn move_right_at_end() {
        let mut input = InputLine::new();
        input.insert('a');
        input.move_right(); // Should stay at 1
        assert_eq!(input.cursor_pos(), 1);
    }

    #[test]
    fn move_home_end() {
        let mut input = InputLine::new();
        input.insert('a');
        input.insert('b');
        input.insert('c');

        input.move_home();
        assert_eq!(input.cursor_pos(), 0);

        input.move_end();
        assert_eq!(input.cursor_pos(), 3);
    }

    #[test]
    fn move_word_left() {
        let mut input = InputLine::new();
        for ch in "hello world foo".chars() {
            input.insert(ch);
        }
        assert_eq!(input.cursor_pos(), 15);

        input.move_word_left();
        assert_eq!(input.cursor_pos(), 12); // before "foo"

        input.move_word_left();
        assert_eq!(input.cursor_pos(), 6); // before "world"

        input.move_word_left();
        assert_eq!(input.cursor_pos(), 0); // before "hello"
    }

    #[test]
    fn move_word_left_at_start() {
        let mut input = InputLine::new();
        input.insert('x');
        input.move_home();
        input.move_word_left(); // Should stay at 0
        assert_eq!(input.cursor_pos(), 0);
    }

    #[test]
    fn move_word_right() {
        let mut input = InputLine::new();
        for ch in "hello world foo".chars() {
            input.insert(ch);
        }
        input.move_home();

        input.move_word_right();
        assert_eq!(input.cursor_pos(), 6); // after "hello "

        input.move_word_right();
        assert_eq!(input.cursor_pos(), 12); // after "world "

        input.move_word_right();
        assert_eq!(input.cursor_pos(), 15); // end
    }

    #[test]
    fn move_word_right_at_end() {
        let mut input = InputLine::new();
        input.insert('x');
        input.move_word_right(); // Should stay at 1
        assert_eq!(input.cursor_pos(), 1);
    }

    #[test]
    fn delete_word_back() {
        let mut input = InputLine::new();
        for ch in "hello world".chars() {
            input.insert(ch);
        }
        input.delete_word_back();
        assert_eq!(input.text(), "hello ");
        assert_eq!(input.cursor_pos(), 6);
    }

    #[test]
    fn delete_word_back_multiple_spaces() {
        let mut input = InputLine::new();
        for ch in "hello   world".chars() {
            input.insert(ch);
        }
        input.delete_word_back();
        assert_eq!(input.text(), "hello   ");
    }

    #[test]
    fn delete_word_back_at_start() {
        let mut input = InputLine::new();
        input.insert('a');
        input.move_home();
        input.delete_word_back(); // Should do nothing
        assert_eq!(input.text(), "a");
    }

    #[test]
    fn clear() {
        let mut input = InputLine::new();
        input.insert('a');
        input.insert('b');
        input.clear();
        assert!(input.is_empty());
        assert_eq!(input.cursor_pos(), 0);
    }

    #[test]
    fn submit_returns_text_and_clears() {
        let mut input = InputLine::new();
        for ch in "status".chars() {
            input.insert(ch);
        }
        let text = input.submit();
        assert_eq!(text, "status");
        assert!(input.is_empty());
    }

    #[test]
    fn submit_adds_to_history() {
        let mut input = InputLine::new();
        for ch in "cmd1".chars() {
            input.insert(ch);
        }
        input.submit();
        assert_eq!(input.history_len(), 1);

        for ch in "cmd2".chars() {
            input.insert(ch);
        }
        input.submit();
        assert_eq!(input.history_len(), 2);
    }

    #[test]
    fn submit_empty_does_not_add_history() {
        let mut input = InputLine::new();
        let text = input.submit();
        assert_eq!(text, "");
        assert_eq!(input.history_len(), 0);
    }

    #[test]
    fn history_navigation() {
        let mut input = InputLine::new();

        // Add some history
        for ch in "first".chars() {
            input.insert(ch);
        }
        input.submit();
        for ch in "second".chars() {
            input.insert(ch);
        }
        input.submit();
        for ch in "third".chars() {
            input.insert(ch);
        }
        input.submit();

        // Navigate up
        input.history_up();
        assert_eq!(input.text(), "third");

        input.history_up();
        assert_eq!(input.text(), "second");

        input.history_up();
        assert_eq!(input.text(), "first");

        // At oldest, should stay
        input.history_up();
        assert_eq!(input.text(), "first");

        // Navigate back down
        input.history_down();
        assert_eq!(input.text(), "second");

        input.history_down();
        assert_eq!(input.text(), "third");

        // Past newest, should return to empty
        input.history_down();
        assert_eq!(input.text(), "");
    }

    #[test]
    fn history_preserves_current_input() {
        let mut input = InputLine::new();

        for ch in "old".chars() {
            input.insert(ch);
        }
        input.submit();

        // Type something new
        for ch in "new".chars() {
            input.insert(ch);
        }

        // Navigate up to history
        input.history_up();
        assert_eq!(input.text(), "old");

        // Navigate back down should restore "new"
        input.history_down();
        assert_eq!(input.text(), "new");
    }

    #[test]
    fn history_up_empty_history() {
        let mut input = InputLine::new();
        input.history_up(); // Should do nothing
        assert!(input.is_empty());
    }

    #[test]
    fn history_down_not_browsing() {
        let mut input = InputLine::new();
        input.history_down(); // Should do nothing
        assert!(input.is_empty());
    }

    #[test]
    fn render_basic() {
        let mut input = InputLine::new();
        for ch in "hello".chars() {
            input.insert(ch);
        }
        let rendered = input.render("cmx> ", 80);
        assert_eq!(rendered, "cmx> hello");
    }

    #[test]
    fn render_empty() {
        let input = InputLine::new();
        let rendered = input.render("$ ", 80);
        assert_eq!(rendered, "$ ");
    }

    #[test]
    fn render_truncates_long_input() {
        let mut input = InputLine::new();
        for ch in "abcdefghijklmnopqrstuvwxyz".chars() {
            input.insert(ch);
        }
        let rendered = input.render("$ ", 10);
        // Should fit within 10 characters
        assert!(rendered.chars().count() <= 10);
    }

    #[test]
    fn is_empty_checks() {
        let mut input = InputLine::new();
        assert!(input.is_empty());
        input.insert('x');
        assert!(!input.is_empty());
        input.delete_back();
        assert!(input.is_empty());
    }

    #[test]
    fn default_is_new() {
        let input = InputLine::default();
        assert!(input.is_empty());
        assert_eq!(input.cursor_pos(), 0);
        assert_eq!(input.history_len(), 0);
    }

    #[test]
    fn insert_unicode() {
        let mut input = InputLine::new();
        input.insert('\u{1F600}'); // emoji
        input.insert('a');
        assert_eq!(input.text(), "\u{1F600}a");
        assert_eq!(input.cursor_pos(), 2);
    }

    #[test]
    fn delete_in_middle() {
        let mut input = InputLine::new();
        for ch in "abcd".chars() {
            input.insert(ch);
        }
        // Move to between b and c
        input.move_left();
        input.move_left();
        // Delete back (removes b)
        input.delete_back();
        assert_eq!(input.text(), "acd");
        assert_eq!(input.cursor_pos(), 1);
    }

    #[test]
    fn delete_forward_in_middle() {
        let mut input = InputLine::new();
        for ch in "abcd".chars() {
            input.insert(ch);
        }
        input.move_home();
        input.move_right(); // after 'a'
        input.delete_forward(); // removes 'b'
        assert_eq!(input.text(), "acd");
        assert_eq!(input.cursor_pos(), 1);
    }

    #[test]
    fn multiple_word_operations() {
        let mut input = InputLine::new();
        for ch in "agent.new worker w1".chars() {
            input.insert(ch);
        }

        // Delete last word
        input.delete_word_back();
        assert_eq!(input.text(), "agent.new worker ");

        // Delete again
        input.delete_word_back();
        assert_eq!(input.text(), "agent.new ");

        // Delete again (includes the dot-separated parts)
        input.delete_word_back();
        // "agent" + "." remain since "." is not alphanumeric
        assert_eq!(input.text(), "agent.");
    }

    #[test]
    fn history_cursor_position_at_end() {
        let mut input = InputLine::new();
        for ch in "hello".chars() {
            input.insert(ch);
        }
        input.submit();

        input.history_up();
        assert_eq!(input.text(), "hello");
        assert_eq!(input.cursor_pos(), 5); // cursor at end
    }
}
