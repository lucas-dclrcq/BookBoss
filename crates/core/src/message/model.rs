use chrono::{DateTime, Utc};

pub type SystemMessageId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MessageSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct NewSystemMessage {
    pub source_task: String,
    pub severity: MessageSeverity,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct SystemMessage {
    pub id: SystemMessageId,
    pub source_task: String,
    pub severity: MessageSeverity,
    pub message: String,
    pub created_at: DateTime<Utc>,
}
