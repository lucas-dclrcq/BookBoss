# Managing Your Library

## Adding Books

BookBoss acquires books through a watched directory. The pipeline runs automatically in the background.

### Workflow

1. **Drop a file** into the directory configured as `BOOKBOSS__IMPORT__WATCH_DIRECTORY`
2. **BookBoss picks it up** — the scanner runs on a configurable interval (default: 60 seconds) and hashes each new file to avoid duplicates
3. **Metadata is extracted** — for EPUB files, embedded metadata is read from the OPF inside the archive; other formats fall through to provider lookup
4. **Provider enrichment** — if the file contains an ISBN, BookBoss queries external metadata providers (Hardcover, Open Library, Google Books) for additional metadata and a cover image
5. **Review queue** — the book lands in the **Incoming** section of the library (requires the _Approve Imports_ capability)

### Reviewing and Approving

Navigate to **Library → Incoming** to see books awaiting review.

Each review page shows three columns: the field name, the current extracted value (editable), and the value fetched from the metadata provider. Use the **←** copy buttons to pull individual fields from the provider into the current value.

- **Fetch provider data** — re-queries the provider using the current identifiers in the form
- **Approve** — commits the edited metadata, moves the book to your library, and sets its status to _Available_
- **Reject** — discards the import
- **Cancel** — returns to the Incoming list without changes

### File Storage

Approved books are stored under `BOOKBOSS__LIBRARY__LIBRARY_PATH` with the layout:

```
{library_path}/
└── BK_<token>/
    ├── <author>-<title>.epub   # the book file (slug derived from author + title)
    ├── cover.jpg               # cover image
    └── metadata.opf            # OPF sidecar with all metadata
```

### Duplicate Detection

Files are SHA-256 hashed before ingestion. If a file with the same hash already exists in the library, the import is skipped automatically.

---

## Browsing Your Library

### Book Grid and Table

The main **Library** view shows your approved books. Books display as a grid of cover thumbnails by default.

Use the **search / filter** controls in the sidebar to narrow by title, author, series, or shelf.

### Book Detail Page

Click any book to open its detail page. From there you can:

- View full metadata — title, authors, series, description, published year, page count, language, and identifiers (ISBN, ASIN, Hardcover, etc.)
- **Download** the book file — format badges (e.g. `EPUB ↓ 2.3 MB`) are download links; click to save the file to your device
- **Edit** metadata — opens the edit page
- **Delete** the book (requires the _Delete Book_ capability)

---

## Editing Metadata

Click **Edit** on any book's detail page to open the metadata editor. You can update:

- Title, description, language, page count, published year
- Authors (with roles: Author, Editor, Translator, Illustrator)
- Series name and number
- Cover image
- Genres, tags, publishers
- Identifiers (ISBN-13, ISBN-10, ASIN, etc.)

Changes are saved to the database and to the OPF sidecar file on disk.

---

## Shelves

Shelves are named collections you create to organise your books.

### Creating a Shelf

Open **Settings → Shelves** (or use the sidebar) to create a new shelf. Give it a name.

### Adding Books to a Shelf

There way to add a book to a shelf:

1. **Drag and drop** — in the book grid, drag a book's cover onto a shelf pill

### Removing Books from a Shelf

Open the shelf, then remove individual books from the shelf view.

---

## Downloading Books

Format download links appear on each book's detail page under the **Formats** section. Each badge shows the format name and file size. Clicking downloads the original file as imported.

> **Note:** Downloads serve the original imported file. Metadata edits made inside BookBoss (title, author, cover) are stored in the database and OPF sidecar but are not written back into the EPUB/MOBI file itself.
