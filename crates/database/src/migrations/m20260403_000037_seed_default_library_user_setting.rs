use bb_core::library::ALL_BOOKS_LIBRARY_TOKEN;
use chrono::Utc;
use sea_orm::{ActiveValue::Set, EntityTrait};
use sea_orm_migration::prelude::*;

use crate::entities::{prelude, user_settings, users};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let all_users = prelude::Users::find().all(db).await?;

        if all_users.is_empty() {
            return Ok(());
        }

        let now = Utc::now();
        let models: Vec<user_settings::ActiveModel> = all_users
            .into_iter()
            .map(|u: users::Model| user_settings::ActiveModel {
                user_id: Set(u.id),
                key: Set("default_library".to_owned()),
                value: Set(ALL_BOOKS_LIBRARY_TOKEN.to_string()),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
            })
            .collect();

        match prelude::UserSettings::insert_many(models)
            .on_conflict(
                OnConflict::columns([user_settings::Column::UserId, user_settings::Column::Key])
                    .do_nothing()
                    .to_owned(),
            )
            .exec(db)
            .await
        {
            Ok(_) | Err(DbErr::RecordNotInserted) => Ok(()),
            Err(e) => Err(e),
        }
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
