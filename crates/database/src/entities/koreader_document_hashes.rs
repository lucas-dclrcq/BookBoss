use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "koreader_document_hashes")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub book_id: i64,
    pub document_hash: String,
    pub created_at: DateTimeWithTimeZone,
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {}
