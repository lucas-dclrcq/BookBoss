use chrono::{DateTime, Utc};

use crate::{
    book::{BookId, FileFormat, FileRole},
    device::DeviceId,
};

#[derive(Debug, Clone)]
pub struct DeviceBook {
    pub device_id: DeviceId,
    pub book_id: BookId,
    pub format: FileFormat,
    pub file_role: FileRole,
    pub synced_at: DateTime<Utc>,
}
