use crate::{
    Error,
    book::{NewTag, Tag, TagId, TagToken},
    repository::Transaction,
};

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait TagRepository: Send + Sync {
    async fn add_tag(&self, transaction: &dyn Transaction, tag: NewTag) -> Result<Tag, Error>;
    async fn update_tag(&self, transaction: &dyn Transaction, tag: Tag) -> Result<Tag, Error>;
    async fn find_by_id(&self, transaction: &dyn Transaction, id: TagId) -> Result<Option<Tag>, Error>;
    async fn find_by_token(&self, transaction: &dyn Transaction, token: TagToken) -> Result<Option<Tag>, Error>;
    async fn find_by_name(&self, transaction: &dyn Transaction, name: &str) -> Result<Option<Tag>, Error>;
    async fn list_tags(&self, transaction: &dyn Transaction, start_id: Option<TagId>, page_size: Option<u64>) -> Result<Vec<Tag>, Error>;
    async fn list_all_tags(&self, transaction: &dyn Transaction) -> Result<Vec<Tag>, Error>;
    async fn delete_tag(&self, transaction: &dyn Transaction, id: TagId) -> Result<(), Error>;
    /// Returns `(Tag, available_count, has_incoming)` where `available_count`
    /// is the number of Available books with this tag, and `has_incoming`
    /// is true if any non-Available book references it.
    async fn list_tags_with_counts(&self, transaction: &dyn Transaction) -> Result<Vec<(Tag, u64, bool)>, Error>;
    /// Deletes all tags not referenced by any book. Returns the number of
    /// deleted rows.
    async fn delete_unused_tags(&self, transaction: &dyn Transaction) -> Result<u64, Error>;
}
