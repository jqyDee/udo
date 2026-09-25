//! Free-text form input: value + cursor, editing and cursor movement.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::keys::is_text_input;

/// Free text with a cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize, // character index
    /// Dim hint shown while `value` is empty (e.g. what empty falls back to).
    pub placeholder: Option<String>,
}

impl TextInput {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self {
            value,
            cursor,
            placeholder: None,
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    // --------------- Editing ---------------

    pub fn insert_char(&mut self, c: char) {
        let byte_offset = self.byte_offset_for_char(self.cursor);
        self.value.insert(byte_offset, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        // early exit for cursor at position 0
        if self.cursor == 0 {
            return;
        }

        self.cursor -= 1;
        let byte_idx = self.byte_offset_for_char(self.cursor);
        self.value.remove(byte_idx);
    }

    pub fn delete(&mut self) {
        let char_count = self.value.chars().count();

        // early exit for cursor at the end of the input string
        if self.cursor >= char_count {
            return;
        }

        let byte_idx = self.byte_offset_for_char(self.cursor);
        self.value.remove(byte_idx);
    }

    // --------------- Moving ---------------

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        let char_count = self.value.chars().count();
        if self.cursor < char_count {
            self.cursor += 1;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.value.chars().count();
    }

    // --------------- Bigger Deletes ---------------

    /// Delete the word left of the cursor (Ctrl+W). Words end at whitespace
    /// and `/`, like in a shell, so in a path only the last part goes:
    /// `a/b/c|` -> `a/b/|`. Separators right before the cursor go with it.
    pub fn delete_word_before(&mut self) {
        let chars: Vec<char> = self.value.chars().collect();
        let cursor = self.cursor.min(chars.len());
        let is_sep = |c: char| c.is_whitespace() || c == '/';

        let mut start = cursor;
        while start > 0 && is_sep(chars[start - 1]) {
            start -= 1;
        }
        while start > 0 && !is_sep(chars[start - 1]) {
            start -= 1;
        }
        self.remove_chars(start, cursor);
        self.cursor = start;
    }

    /// Delete everything left of the cursor (Ctrl+U).
    pub fn delete_to_start(&mut self) {
        self.remove_chars(0, self.cursor);
        self.cursor = 0;
    }

    /// Delete everything from the cursor on (Ctrl+K).
    pub fn delete_to_end(&mut self) {
        let char_count = self.value.chars().count();
        self.remove_chars(self.cursor.min(char_count), char_count);
    }

    // --------------- Keys ---------------

    /// Typing and cursor keys, plus shell-like Ctrl shortcuts: W word, U to
    /// start, K to end (delete); A / E jump to start / end. Other keys and
    /// other Ctrl/Alt combos are ignored.
    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) if key.modifiers == KeyModifiers::CONTROL => match c {
                'w' => self.delete_word_before(),
                'u' => self.delete_to_start(),
                'k' => self.delete_to_end(),
                'a' => self.move_home(),
                'e' => self.move_end(),
                _ => {}
            },
            KeyCode::Char(c) if is_text_input(key) => self.insert_char(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Home => self.move_home(),
            KeyCode::End => self.move_end(),
            _ => {}
        }
    }

    // --------------- Helpers ---------------

    /// Remove chars `from..to` (char indices). Cursor is left to the caller.
    fn remove_chars(&mut self, from: usize, to: usize) {
        let range = self.byte_offset_for_char(from)..self.byte_offset_for_char(to);
        self.value.replace_range(range, "");
    }

    fn byte_offset_for_char(&self, char_idx: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_idx)
            .map_or(self.value.len(), |(byte_offset, _)| byte_offset)
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;

    use super::*;
    use crate::test_util::press;

    fn form_field_special() -> TextInput {
        TextInput {
            value: "öüä".to_string(),
            cursor: 1,
            placeholder: None,
        }
    }

    // --------------- Editing Tests ---------------

    #[test]
    fn insert_char() {
        let mut field = TextInput::new("test");
        field.cursor = 0;
        let char_count = field.value.chars().count();
        field.insert_char('b');
        assert_eq!(field.value, "btest");
        assert_eq!(field.cursor, 1);
        assert_eq!(field.value.chars().count(), char_count + 1);
    }

    #[test]
    fn insert_char_at_end() {
        let mut field = TextInput::new("hello");
        assert_eq!(field.cursor, 5);
        field.insert_char('!');
        assert_eq!(field.value, "hello!");
        assert_eq!(field.cursor, 6);
    }

    #[test]
    fn insert_char_special_chars() {
        let mut field = form_field_special();
        let char_count = field.value.chars().count();
        field.insert_char('b');
        assert_eq!(field.value, "öbüä");
        assert_eq!(field.cursor, 2);
        assert_eq!(field.value.chars().count(), char_count + 1);
    }

    #[test]
    fn backspace_from_middle() {
        let mut field = TextInput {
            value: "test".to_string(),
            cursor: 2,
            placeholder: None,
        };
        field.backspace();
        assert_eq!(field.value, "tst");
        assert_eq!(field.cursor, 1);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let mut field = TextInput {
            value: "test".to_string(),
            cursor: 0,
            placeholder: None,
        };
        field.backspace();
        assert_eq!(field.value, "test");
        assert_eq!(field.cursor, 0);
    }

    #[test]
    fn backspace_special_chars() {
        let mut field = form_field_special(); // "öüä", cursor at 1
        field.backspace();
        assert_eq!(field.value, "üä");
        assert_eq!(field.cursor, 0);
    }

    #[test]
    fn delete_from_middle() {
        let mut field = TextInput {
            value: "test".to_string(),
            cursor: 1,
            placeholder: None,
        };
        field.delete();
        assert_eq!(field.value, "tst");
        assert_eq!(field.cursor, 1);
    }

    #[test]
    fn delete_at_end_is_noop() {
        let mut field = TextInput::new("test");
        field.delete();
        assert_eq!(field.value, "test");
        assert_eq!(field.cursor, 4);
    }

    #[test]
    fn delete_special_chars() {
        let mut field = form_field_special(); // "öüä", cursor at 1
        field.delete();
        assert_eq!(field.value, "öä");
        assert_eq!(field.cursor, 1);
    }

    // --------------- Movement Tests ---------------

    #[test]
    fn move_left() {
        let mut field = TextInput {
            value: "test".to_string(),
            cursor: 2,
            placeholder: None,
        };
        field.move_left();
        assert_eq!(field.cursor, 1);
    }

    #[test]
    fn move_left_at_zero_stays_zero() {
        let mut field = TextInput {
            value: "test".to_string(),
            cursor: 0,
            placeholder: None,
        };
        field.move_left();
        assert_eq!(field.cursor, 0);
    }

    #[test]
    fn move_right() {
        let mut field = TextInput {
            value: "test".to_string(),
            cursor: 1,
            placeholder: None,
        };
        field.move_right();
        assert_eq!(field.cursor, 2);
    }

    #[test]
    fn move_right_stops_at_end() {
        let mut field = TextInput::new("test");
        field.move_right();
        assert_eq!(field.cursor, 4);
    }

    #[test]
    fn move_home() {
        let mut field = TextInput::new("test");
        field.move_home();
        assert_eq!(field.cursor, 0);
    }

    #[test]
    fn move_end() {
        let mut field = TextInput {
            value: "test".to_string(),
            cursor: 1,
            placeholder: None,
        };
        field.move_end();
        assert_eq!(field.cursor, 4);
    }

    #[test]
    fn move_end_empty_string() {
        let mut field = TextInput::new("");
        field.move_end();
        assert_eq!(field.cursor, 0);
    }

    // --------------- Key Tests ---------------

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn handle_key_types_and_edits() {
        let mut t = TextInput::new("");
        for c in "abc".chars() {
            t.handle_key(press(KeyCode::Char(c)));
        }
        t.handle_key(press(KeyCode::Left));
        t.handle_key(press(KeyCode::Backspace));
        assert_eq!(t.value, "ac");
        assert_eq!(t.cursor, 1);
    }

    #[test]
    fn handle_key_ignores_ctrl_letters() {
        let mut t = TextInput::new("x");
        t.handle_key(ctrl('c'));
        assert_eq!(t.value, "x");
    }

    // --------------- Bigger Delete Tests ---------------

    #[test]
    fn ctrl_w_deletes_word_and_spaces_before_cursor() {
        let mut t = TextInput::new("read chapter  ");
        t.handle_key(ctrl('w'));
        assert_eq!(t.value, "read ");
        assert_eq!(t.cursor, 5);
        t.handle_key(ctrl('w'));
        assert_eq!(t.value, "");
        t.handle_key(ctrl('w')); // empty: no-op
        assert_eq!((t.value.as_str(), t.cursor), ("", 0));
    }

    #[test]
    fn ctrl_w_stops_at_slash_in_paths() {
        let mut t = TextInput::new("~/uni/cs101/");
        t.handle_key(ctrl('w'));
        assert_eq!(t.value, "~/uni/");
    }

    #[test]
    fn ctrl_w_in_the_middle_keeps_the_rest() {
        let mut t = TextInput::new("öäü wörd rest");
        t.cursor = 8; // after "wörd"
        t.handle_key(ctrl('w'));
        assert_eq!(t.value, "öäü  rest");
        assert_eq!(t.cursor, 4);
    }

    #[test]
    fn ctrl_u_and_ctrl_k_delete_to_start_and_end() {
        let mut t = TextInput::new("hello world");
        t.cursor = 5;
        t.handle_key(ctrl('k'));
        assert_eq!((t.value.as_str(), t.cursor), ("hello", 5));
        t.cursor = 2;
        t.handle_key(ctrl('u'));
        assert_eq!((t.value.as_str(), t.cursor), ("llo", 0));
    }

    #[test]
    fn ctrl_a_and_ctrl_e_jump() {
        let mut t = TextInput::new("abc");
        t.handle_key(ctrl('a'));
        assert_eq!(t.cursor, 0);
        t.handle_key(ctrl('e'));
        assert_eq!(t.cursor, 3);
    }
}
