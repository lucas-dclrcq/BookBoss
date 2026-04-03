pub mod model;
pub mod repository;
pub mod service;

pub use model::{ALL_BOOKS_LIBRARY_ID, ALL_BOOKS_LIBRARY_TOKEN, Library, LibraryId, LibraryToken, NewLibrary, all_books_library_token};
pub use repository::LibraryRepository;
#[cfg(test)]
pub use repository::MockLibraryRepository;
#[cfg(any(test, feature = "test-support"))]
pub use service::MockLibraryService;
pub use service::{LibraryEntry, LibraryService, LibraryServiceImpl};

use crate::{
    repository::Transaction,
    user::{UserId, repository::user_settings::UserSettingRepository},
};

/// Look up the user's effective library_id from their default_library setting.
/// Falls back to [`ALL_BOOKS_LIBRARY_ID`] if the setting is absent or the
/// library doesn't exist.
pub async fn resolve_user_default_library(
    transaction: &dyn Transaction,
    user_setting_repository: &dyn UserSettingRepository,
    library_repository: &dyn LibraryRepository,
    user_id: UserId,
) -> Result<LibraryId, crate::Error> {
    let library_id = if let Some(setting) = user_setting_repository.get(transaction, user_id, "default_library").await? {
        if let Ok(token) = LibraryToken::parse(&setting.value) {
            library_repository
                .find_by_token(transaction, token)
                .await?
                .map_or(ALL_BOOKS_LIBRARY_ID, |l| l.id)
        } else {
            ALL_BOOKS_LIBRARY_ID
        }
    } else {
        ALL_BOOKS_LIBRARY_ID
    };
    Ok(library_id)
}
