//! Free-text form input: value + cursor, editing and cursor movement.

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

    // --------------- Helpers ---------------

    fn byte_offset_for_char(&self, char_idx: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_idx)
            .map_or(self.value.len(), |(byte_offset, _)| byte_offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
