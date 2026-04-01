pub mod fingerprint;
pub mod model;
pub mod repository;
pub mod service;

pub use fingerprint::compute_sidecar_fingerprint;
pub use model::{
    Author, AuthorId, AuthorRole, AuthorToken, Book, BookAuthor, BookFile, BookHydrationData, BookId, BookIdentifier, BookQuery, BookSortField, BookSortOrder,
    BookStatus, BookToken, FileFormat, FileRole, Genre, GenreId, GenreToken, IdentifierType, MetadataSource, NewAuthor, NewBook, NewGenre, NewPublisher,
    NewSeries, NewTag, Publisher, PublisherId, PublisherToken, Series, SeriesId, SeriesToken, SortDirection, Tag, TagId, TagToken, book_filename, book_slug,
};
pub use repository::{AuthorRepository, BookRepository, GenreRepository, PublisherRepository, SeriesRepository, TagRepository};
pub use service::BookService;
pub(crate) use service::BookServiceImpl;
