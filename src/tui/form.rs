use std::{ops::Range, path::PathBuf};

use chrono::{Local, Months, NaiveDateTime, NaiveTime, TimeDelta};

use crate::model::{container::ContainerKind, tree::NodePath};

/// Display format of a date field. Fixed width and ASCII only, so byte
/// ranges == char ranges (see `Segment::range`).
pub const DATE_FMT: &str = "%Y-%m-%d %H:%M";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    pub label: &'static str,
    pub input: FieldInput,
}

/// What kind of value a field holds, and so which keys edit it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldInput {
    Text(TextInput),
    Date(DateInput),
}

/// Free text with a cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize, // character index
    /// Dim hint shown while `value` is empty (e.g. what empty falls back to).
    pub placeholder: Option<String>,
}

/// Local date + time, edited one segment at a time (←/→ pick, ↑/↓ change).
/// Local, not UTC: converted only on submit (`App::submit_form`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateInput {
    pub value: NaiveDateTime,
    pub segment: Segment,
}

/// One editable part of a `DATE_FMT` string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    Year,
    Month,
    Day,
    Hour,
    Minute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormAction {
    CreateTask {
        parent: NodePath,
    },
    CreateContainer {
        parent: NodePath,
        kind: ContainerKind,
    },
    EditNode {
        path: NodePath,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    pub title: String,
    pub fields: Vec<FormField>,
    pub active_field: usize,
    pub action: FormAction,
}

impl FormField {
    /// Text field, cursor at the end of `value`.
    pub fn text(label: &'static str, value: impl Into<String>) -> Self {
        Self {
            label,
            input: FieldInput::Text(TextInput::new(value)),
        }
    }

    /// Date field, starting on `Segment::Day` (the part changed most).
    pub fn date(label: &'static str, value: NaiveDateTime) -> Self {
        Self {
            label,
            input: FieldInput::Date(DateInput::new(value)),
        }
    }
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

impl Segment {
    /// Segment to the right.
    pub fn next(self) -> Self {
        match self {
            Self::Year => Self::Month,
            Self::Month => Self::Day,
            Self::Day => Self::Hour,
            Self::Hour => Self::Minute,
            Self::Minute => Self::Year,
        }
    }

    /// Segment to the left.
    pub fn prev(self) -> Self {
        match self {
            Self::Minute => Self::Hour,
            Self::Hour => Self::Day,
            Self::Day => Self::Month,
            Self::Month => Self::Year,
            Self::Year => Self::Minute,
        }
    }

    /// Where the segment sits in a `DATE_FMT` string.
    pub fn range(self) -> Range<usize> {
        match self {
            Self::Year => 0..4,
            Self::Month => 5..7,
            Self::Day => 8..10,
            Self::Hour => 11..13,
            Self::Minute => 14..16,
        }
    }
}

impl DateInput {
    /// Starts on `Segment::Day`.
    pub fn new(value: NaiveDateTime) -> Self {
        Self {
            value,
            segment: Segment::Day,
        }
    }

    pub fn next_segment(&mut self) {
        self.segment = self.segment.next();
    }

    pub fn prev_segment(&mut self) {
        self.segment = self.segment.prev();
    }

    /// +1 / -1 on the current segment (minutes: ±5).
    /// Year/month: `checked_add_months` / `checked_sub_months` (clamps the
    /// day, Jan 31 + 1 month = Feb 28/29). Day/hour/minute:
    /// `checked_add_signed(TimeDelta)` (carries, 23:55 + 5 min = next day
    /// 00:00). Always `checked_*`, never `+`: `+` panics on overflow, here
    /// overflow -> unchanged.
    pub fn step(&mut self, up: bool) {
        let sign: i64 = if up { 1 } else { -1 };

        let stepped = match self.segment {
            Segment::Year => self.shift_months(12, up),
            Segment::Month => self.shift_months(1, up),
            Segment::Day => self.value.checked_add_signed(TimeDelta::days(sign)),
            Segment::Hour => self.value.checked_add_signed(TimeDelta::hours(sign)),
            Segment::Minute => self.value.checked_add_signed(TimeDelta::minutes(sign * 5)),
        };

        self.value = stepped.unwrap_or(self.value);
    }

    fn shift_months(&self, count: u32, up: bool) -> Option<NaiveDateTime> {
        if up {
            self.value.checked_add_months(Months::new(count))
        } else {
            self.value.checked_sub_months(Months::new(count))
        }
    }

    /// Date -> today, time kept (`t` key).
    pub fn set_today(&mut self) {
        self.value = Local::now().date_naive().and_time(self.value.time());
    }

    pub fn display(&self) -> String {
        self.value.format(DATE_FMT).to_string()
    }
}

impl Form {
    /// Due defaults to tomorrow 12:00 local, so a new task isn't overdue.
    pub fn new_task(parent: NodePath, parent_name: &str) -> Self {
        let tomorrow_noon = (Local::now().date_naive() + TimeDelta::days(1))
            .and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap());
        Self::new_task_with_due(parent, parent_name, tomorrow_noon)
    }

    /// `due` is local time (see `DateInput`).
    pub fn new_task_with_due(parent: NodePath, parent_name: &str, due: NaiveDateTime) -> Self {
        Self {
            title: format!("new task · in {parent_name}"),
            fields: vec![FormField::text("name", ""), FormField::date("due", due)],
            active_field: 0,
            action: FormAction::CreateTask { parent },
        }
    }

    pub fn new_container(
        parent: NodePath,
        parent_name: &str,
        default_dir: Option<PathBuf>,
        kind: ContainerKind,
    ) -> Self {
        // Empty dir = `<parent dir>/<name>` (see `App::submit_form`). Not
        // prefilled: the parent dir itself already holds a `.udo.toml`.
        let dir = TextInput::new("");
        let dir = match default_dir {
            Some(d) => dir.with_placeholder(format!("{}/<name>", d.display())),
            None => dir,
        };
        Self {
            title: format!("new {:?} · in {parent_name}", kind),
            fields: vec![
                FormField::text("name", ""),
                FormField {
                    label: "dir",
                    input: FieldInput::Text(dir),
                },
            ],
            active_field: 0,
            action: FormAction::CreateContainer { parent, kind },
        }
    }

    pub fn active_field_mut(&mut self) -> Option<&mut FormField> {
        self.fields.get_mut(self.active_field)
    }

    pub fn next_field(&mut self) -> &mut FormField {
        let field_count = self.fields.len();
        assert!(field_count > 0, "Form has no fields");
        self.active_field = (self.active_field + 1) % field_count;
        &mut self.fields[self.active_field]
    }

    pub fn prev_field(&mut self) -> &mut FormField {
        let field_count = self.fields.len();
        assert!(field_count > 0, "Form has no fields");
        self.active_field = (self.active_field + field_count - 1) % field_count;
        &mut self.fields[self.active_field]
    }

    /// Value of the text field `label`. None if missing or not a text field.
    pub fn text_value(&self, label: &str) -> Option<&str> {
        match &self.fields.iter().find(|f| f.label == label)?.input {
            FieldInput::Text(t) => Some(t.value.as_str()),
            FieldInput::Date(_) => None,
        }
    }

    /// Value of the date field `label`. None if missing or not a date field.
    pub fn date_value(&self, label: &str) -> Option<NaiveDateTime> {
        match &self.fields.iter().find(|f| f.label == label)?.input {
            FieldInput::Date(d) => Some(d.value),
            FieldInput::Text(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::NaiveDate;

    use super::*;

    fn form_field_special() -> TextInput {
        TextInput {
            value: "öüä".to_string(),
            cursor: 1,
            placeholder: None,
        }
    }

    fn test_form() -> Form {
        Form {
            title: "Test Form".into(),
            fields: vec![
                FormField::text("Field1", "val1"),
                FormField::text("Field2", "val2"),
                FormField::text("Field3", "val3"),
            ],
            active_field: 0,
            action: FormAction::CreateTask { parent: vec![] },
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

    // --------------- Form Navigation & Constructors Tests ---------------

    #[test]
    fn form_next_field_advances_and_wraps() {
        let mut form = test_form();
        assert_eq!(form.active_field, 0);

        let f = form.next_field();
        assert_eq!(f.label, "Field2");
        assert_eq!(form.active_field, 1);

        let f = form.next_field();
        assert_eq!(f.label, "Field3");
        assert_eq!(form.active_field, 2);

        // wraps back to 0
        let f = form.next_field();
        assert_eq!(f.label, "Field1");
        assert_eq!(form.active_field, 0);
    }

    #[test]
    fn form_prev_field_wraps_from_zero_and_steps_back() {
        let mut form = test_form();
        assert_eq!(form.active_field, 0);

        // prev from 0 wraps to last field without underflowing
        let f = form.prev_field();
        assert_eq!(f.label, "Field3");
        assert_eq!(form.active_field, 2);

        let f = form.prev_field();
        assert_eq!(f.label, "Field2");
        assert_eq!(form.active_field, 1);

        let f = form.prev_field();
        assert_eq!(f.label, "Field1");
        assert_eq!(form.active_field, 0);
    }

    #[test]
    fn form_active_field_mut() {
        let mut form = test_form();
        assert_eq!(form.active_field_mut().unwrap().label, "Field1");
        form.next_field();
        assert_eq!(form.active_field_mut().unwrap().label, "Field2");
    }

    #[test]
    fn new_task_initialization() {
        let fixed_date = NaiveDate::from_ymd_opt(2026, 10, 15)
            .unwrap()
            .and_hms_opt(14, 30, 0)
            .unwrap();
        let form = Form::new_task_with_due(vec![0], "CS101", fixed_date);

        assert_eq!(form.title, "new task · in CS101");
        assert_eq!(form.active_field, 0);
        assert_eq!(form.action, FormAction::CreateTask { parent: vec![0] });
        assert_eq!(form.text_value("name"), Some(""));
        assert_eq!(form.date_value("due"), Some(fixed_date));
    }

    #[test]
    fn new_container_workspace() {
        let form = Form::new_container(vec![], "root", None, ContainerKind::Workspace);

        assert_eq!(form.title, "new Workspace · in root");
        assert_eq!(form.active_field, 0);
        assert_eq!(
            form.action,
            FormAction::CreateContainer {
                parent: vec![],
                kind: ContainerKind::Workspace
            }
        );
        assert_eq!(form.text_value("name"), Some(""));
        assert_eq!(form.text_value("dir"), Some(""));
    }

    #[test]
    fn new_container_project_with_default_dir() {
        let default_dir = PathBuf::from("/home/user/workspace/project");
        let form = Form::new_container(
            vec![1],
            "uni",
            Some(default_dir.clone()),
            ContainerKind::Project,
        );

        assert_eq!(form.title, "new Project · in uni");
        assert_eq!(
            form.action,
            FormAction::CreateContainer {
                parent: vec![1],
                kind: ContainerKind::Project
            }
        );
        assert_eq!(form.text_value("name"), Some(""));
        assert_eq!(form.text_value("dir"), Some(""));
        let FieldInput::Text(dir) = &form.fields[1].input else {
            panic!("dir is not a text field");
        };
        assert_eq!(
            dir.placeholder.as_deref(),
            Some("/home/user/workspace/project/<name>")
        );
    }

    // --------------- Segment Tests ---------------

    const SEGMENTS: [Segment; 5] = [
        Segment::Year,
        Segment::Month,
        Segment::Day,
        Segment::Hour,
        Segment::Minute,
    ];

    #[test]
    fn segment_next_goes_left_to_right() {
        assert_eq!(Segment::Year.next(), Segment::Month);
        assert_eq!(Segment::Month.next(), Segment::Day);
        assert_eq!(Segment::Day.next(), Segment::Hour);
        assert_eq!(Segment::Hour.next(), Segment::Minute);
    }

    #[test]
    fn segment_next_wraps_from_minute_to_year() {
        assert_eq!(Segment::Minute.next(), Segment::Year);
    }

    #[test]
    fn segment_prev_goes_right_to_left() {
        assert_eq!(Segment::Minute.prev(), Segment::Hour);
        assert_eq!(Segment::Hour.prev(), Segment::Day);
        assert_eq!(Segment::Day.prev(), Segment::Month);
        assert_eq!(Segment::Month.prev(), Segment::Year);
    }

    #[test]
    fn segment_prev_wraps_from_year_to_minute() {
        assert_eq!(Segment::Year.prev(), Segment::Minute);
    }

    #[test]
    fn segment_prev_undoes_next() {
        for s in SEGMENTS {
            assert_eq!(s.next().prev(), s, "{s:?}");
            assert_eq!(s.prev().next(), s, "{s:?}");
        }
    }

    #[test]
    fn segment_range_slices_formatted_date() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 7)
            .unwrap()
            .and_hms_opt(9, 5, 0)
            .unwrap();
        // real DATE_FMT output, zero padding included
        let s = date.format(DATE_FMT).to_string();
        assert_eq!(s, "2026-03-07 09:05");

        assert_eq!(&s[Segment::Year.range()], "2026");
        assert_eq!(&s[Segment::Month.range()], "03");
        assert_eq!(&s[Segment::Day.range()], "07");
        assert_eq!(&s[Segment::Hour.range()], "09");
        assert_eq!(&s[Segment::Minute.range()], "05");
    }

    #[test]
    fn segment_ranges_do_not_overlap_and_stay_in_bounds() {
        let len = "2026-03-07 09:05".len();
        for pair in SEGMENTS.windows(2) {
            let (a, b) = (pair[0].range(), pair[1].range());
            assert!(a.end < b.start, "{:?} and {:?} overlap", pair[0], pair[1]);
        }
        assert!(Segment::Minute.range().end <= len);
    }

    // --------------- DateInput Tests ---------------

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    /// `value` stepped once on `segment`.
    fn stepped(value: NaiveDateTime, segment: Segment, up: bool) -> NaiveDateTime {
        let mut d = DateInput { value, segment };
        d.step(up);
        assert_eq!(d.segment, segment, "step must not change the segment");
        d.value
    }

    #[test]
    fn date_input_starts_on_day() {
        let d = DateInput::new(dt(2026, 10, 15, 14, 30));
        assert_eq!(d.segment, Segment::Day);
        assert_eq!(d.value, dt(2026, 10, 15, 14, 30));
    }

    #[test]
    fn date_input_display_uses_date_fmt() {
        let d = DateInput::new(dt(2026, 3, 7, 9, 5));
        assert_eq!(d.display(), "2026-03-07 09:05");
    }

    #[test]
    fn date_input_segment_moves_both_ways() {
        let mut d = DateInput::new(dt(2026, 10, 15, 14, 30)); // on Day
        d.next_segment();
        assert_eq!(d.segment, Segment::Hour);
        d.prev_segment();
        d.prev_segment();
        assert_eq!(d.segment, Segment::Month);
    }

    #[test]
    fn step_each_segment_up_and_down() {
        let v = dt(2026, 6, 15, 12, 30);
        assert_eq!(stepped(v, Segment::Year, true), dt(2027, 6, 15, 12, 30));
        assert_eq!(stepped(v, Segment::Year, false), dt(2025, 6, 15, 12, 30));
        assert_eq!(stepped(v, Segment::Month, true), dt(2026, 7, 15, 12, 30));
        assert_eq!(stepped(v, Segment::Month, false), dt(2026, 5, 15, 12, 30));
        assert_eq!(stepped(v, Segment::Day, true), dt(2026, 6, 16, 12, 30));
        assert_eq!(stepped(v, Segment::Day, false), dt(2026, 6, 14, 12, 30));
        assert_eq!(stepped(v, Segment::Hour, true), dt(2026, 6, 15, 13, 30));
        assert_eq!(stepped(v, Segment::Hour, false), dt(2026, 6, 15, 11, 30));
        assert_eq!(stepped(v, Segment::Minute, true), dt(2026, 6, 15, 12, 35));
        assert_eq!(stepped(v, Segment::Minute, false), dt(2026, 6, 15, 12, 25));
    }

    #[test]
    fn step_day_carries_into_next_month_and_year() {
        assert_eq!(
            stepped(dt(2026, 1, 31, 12, 0), Segment::Day, true),
            dt(2026, 2, 1, 12, 0)
        );
        assert_eq!(
            stepped(dt(2026, 12, 31, 12, 0), Segment::Day, true),
            dt(2027, 1, 1, 12, 0)
        );
    }

    #[test]
    fn step_month_clamps_to_month_end() {
        // leap year / normal year
        assert_eq!(
            stepped(dt(2028, 1, 31, 12, 0), Segment::Month, true),
            dt(2028, 2, 29, 12, 0)
        );
        assert_eq!(
            stepped(dt(2027, 1, 31, 12, 0), Segment::Month, true),
            dt(2027, 2, 28, 12, 0)
        );
        // down as well
        assert_eq!(
            stepped(dt(2026, 3, 31, 12, 0), Segment::Month, false),
            dt(2026, 2, 28, 12, 0)
        );
    }

    #[test]
    fn step_month_carries_across_year() {
        assert_eq!(
            stepped(dt(2026, 12, 15, 12, 0), Segment::Month, true),
            dt(2027, 1, 15, 12, 0)
        );
        assert_eq!(
            stepped(dt(2026, 1, 15, 12, 0), Segment::Month, false),
            dt(2025, 12, 15, 12, 0)
        );
    }

    #[test]
    fn step_year_from_leap_day_clamps() {
        assert_eq!(
            stepped(dt(2028, 2, 29, 12, 0), Segment::Year, true),
            dt(2029, 2, 28, 12, 0)
        );
    }

    #[test]
    fn step_minute_carries_past_midnight() {
        assert_eq!(
            stepped(dt(2026, 10, 15, 23, 55), Segment::Minute, true),
            dt(2026, 10, 16, 0, 0)
        );
    }

    #[test]
    fn step_hour_carries_back_past_midnight() {
        assert_eq!(
            stepped(dt(2026, 10, 15, 0, 30), Segment::Hour, false),
            dt(2026, 10, 14, 23, 30)
        );
    }

    #[test]
    fn step_at_end_of_calendar_leaves_value_unchanged() {
        let max = NaiveDateTime::MAX;
        for s in SEGMENTS {
            assert_eq!(stepped(max, s, true), max, "{s:?} up at MAX");
        }
        let min = NaiveDateTime::MIN;
        for s in SEGMENTS {
            assert_eq!(stepped(min, s, false), min, "{s:?} down at MIN");
        }
    }

    #[test]
    fn set_today_keeps_time_and_segment() {
        let mut d = DateInput {
            value: dt(2000, 1, 1, 9, 45),
            segment: Segment::Hour,
        };

        let before = Local::now().date_naive();
        d.set_today();
        let after = Local::now().date_naive();

        // before/after: don't flake if the test runs across midnight
        assert!(d.value.date() == before || d.value.date() == after);
        assert_eq!(d.value.time(), dt(2000, 1, 1, 9, 45).time());
        assert_eq!(d.segment, Segment::Hour);
    }

    // --------------- Date Field Tests ---------------

    #[test]
    fn form_field_date_wraps_date_input() {
        let f = FormField::date("due", dt(2026, 10, 15, 14, 30));
        assert_eq!(f.label, "due");
        assert_eq!(
            f.input,
            FieldInput::Date(DateInput::new(dt(2026, 10, 15, 14, 30)))
        );
    }

    #[test]
    fn value_getters_only_match_their_own_kind() {
        let form = Form::new_task_with_due(vec![0], "CS101", dt(2026, 10, 15, 14, 30));
        assert_eq!(form.text_value("due"), None); // due is a date
        assert_eq!(form.date_value("name"), None); // name is text
        assert_eq!(form.date_value("missing"), None);
        assert_eq!(form.text_value("missing"), None);
    }
}
