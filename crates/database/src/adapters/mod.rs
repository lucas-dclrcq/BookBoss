pub(crate) mod app_setting;
pub(crate) mod author;
pub(crate) mod book;
pub(crate) mod collection;
pub(crate) mod device;
pub(crate) mod genre;
pub(crate) mod import_job;
pub(crate) mod job;
pub(crate) mod koreader_document_hash;
pub(crate) mod library;
pub(crate) mod publisher;
pub(crate) mod series;
pub(crate) mod session;
pub(crate) mod shelf;
pub(crate) mod system_message;
pub(crate) mod tag;
pub(crate) mod user;
pub(crate) mod user_book_metadata;
pub(crate) mod user_settings;

/// Case-insensitive equality filter on a `name` column.
///
/// Produces `LOWER(col) = LOWER(name)`, matching the pattern used by
/// `find_by_name` in author, genre, publisher, series, and tag adapters.
pub(crate) fn lower_name_eq<C>(col: C, name: &str) -> sea_orm::sea_query::SimpleExpr
where
    C: sea_orm::sea_query::IntoColumnRef,
{
    use sea_orm::{
        ExprTrait,
        sea_query::{BinOper, Expr, Func},
    };
    Expr::expr(Func::lower(Expr::col(col))).binary(BinOper::Equal, Expr::value(name.to_lowercase()))
}
