use crate::{Result, storage::ThreadInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorKind {
    Authentication,
    HistoryExpired,
    Missing,
    Permanent,
}

#[derive(Debug)]
pub struct ProviderError(pub ProviderErrorKind, pub &'static str);

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.1)
    }
}
impl std::error::Error for ProviderError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRef {
    pub id: String,
    pub thread_id: String,
}

#[derive(Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_page: Option<String>,
}

#[derive(Debug)]
pub struct HistoryPage {
    pub changed: Vec<MessageRef>,
    pub next_page: Option<String>,
    pub history_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageState {
    Actionable,
    Inactive,
    Missing,
}

pub trait MailProvider: Sync {
    fn retries(&self) -> u32 {
        0
    }
    fn current_history_id(&self) -> Result<String>;
    fn unread_page(&self, page: Option<&str>, limit: u32) -> Result<Page<MessageRef>>;
    fn history_page(&self, start: &str, page: Option<&str>) -> Result<HistoryPage>;
    fn message_state(&self, id: &str) -> Result<MessageState>;
    fn thread(&self, id: &str) -> Result<ThreadInput>;
    fn attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>>;
    fn raw(&self, message_id: &str) -> Result<Vec<u8>>;
}
