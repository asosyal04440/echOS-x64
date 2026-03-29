pub use super::super::services::{StoreCommand, StoreResponse};

#[inline]
pub fn request_store_sync(app_id: u32, command: StoreCommand) -> Option<StoreResponse> {
    super::super::ipc::request_store_sync(app_id, command)
}
