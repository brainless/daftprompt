//! Workspace file storage.

/// A quota in the storage layer is the maximum number of bytes a workspace may
/// keep. Uploads that would exceed the quota are refused; nothing is billed.
pub struct Quota {
    pub max_bytes: u64,
    pub used_bytes: u64,
}

impl Quota {
    pub fn allows(&self, upload_bytes: u64) -> bool {
        self.used_bytes + upload_bytes <= self.max_bytes
    }
}
