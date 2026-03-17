use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};

use crate::device::DeviceId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncStatus {
    Running,
    Completed,
    Failed,
}

impl SyncStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncStatus::Running => "running",
            SyncStatus::Completed => "completed",
            SyncStatus::Failed => "failed",
        }
    }
}

impl fmt::Display for SyncStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SyncStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "running" => Ok(SyncStatus::Running),
            "completed" => Ok(SyncStatus::Completed),
            "failed" => Ok(SyncStatus::Failed),
            _ => Err(format!("unknown sync status: {s}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewDeviceSyncLog {
    pub device_id: DeviceId,
    pub status: SyncStatus,
    pub books_added: i32,
    pub books_removed: i32,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct DeviceSyncLog {
    pub id: i64,
    pub device_id: DeviceId,
    pub status: SyncStatus,
    pub books_added: i32,
    pub books_removed: i32,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}
