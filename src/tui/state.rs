use ratatui::widgets::ListState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Info(String),
    Error(String),
}

#[derive(Default)]
pub struct UiState {
    pub list: ListState,
    pub status: Option<Status>,
    /// Key help overlay open (`?`). Any key closes it.
    pub show_help: bool,
}

impl UiState {
    pub fn error(&mut self, msg: impl Into<String>) {
        self.status = Some(Status::Error(msg.into()));
    }
    pub fn info(&mut self, msg: impl Into<String>) {
        self.status = Some(Status::Info(msg.into()));
    }
}
