use chrono::{DateTime, Utc};

use crate::{
    book::{BookId, FileFormat},
    device::DeviceId,
};

#[derive(Debug, Clone)]
pub struct DeviceBook {
    pub device_id: DeviceId,
    pub book_id: BookId,
    pub format: FileFormat,
    /// ID of the `book_files` record that was sent to the device.
    /// Stored as a plain integer with no FK constraint so that deleting
    /// the file or device does not cascade-affect this record.
    pub book_file_id: i64,
    pub synced_at: DateTime<Utc>,
}
