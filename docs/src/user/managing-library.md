# Managing Your Library

## Adding Books

BookBoss acquires books through a bookdrop directory. The pipeline runs automatically in the background.

### Workflow

1. **Drop a file** into the directory configured as `BOOKBOSS__IMPORT__BOOKDROP_PATH`, or
   **drag and drop an EPUB** onto the **Library → Incoming** page in the web UI. Uploads
   through the UI are written into the bookdrop directory and pick up the same way as files
   placed there directly. The maximum upload size is 70 MiB.
2. **BookBoss picks it up** — the scanner runs on a configurable interval (default: 60 seconds) and hashes each new file to avoid duplicates. You can also trigger a scan manually from the UI.
3. **Metadata is extracted** — for EPUB files, embedded metadata is read from the OPF inside the archive; other formats fall through to provider lookup
4. **Provider enrichment** — BookBoss queries external metadata providers (Hardcover, Open Library, Google Books) in parallel, using title+author similarity scoring to select the best match. Cover art is fetched from the most confident provider.
5. **Review queue** — the book lands in the **Incoming** section of the library (requires the _Approve Imports_ capability)

### Reviewing and Approving

Navigate to **Library > Incoming** to see books awaiting review.

Each review page shows three columns: the field name, the current extracted value (editable), and the value fetched from the metadata provider. Use the copy buttons to pull individual fields from the provider into the current value.

- **Fetch provider data** — re-queries the provider using the current identifiers in the form
- **Libraries** — assign the book to one or more libraries at approval time (requires custom libraries to exist)
- **Approve** — commits the edited metadata, moves the book to your library, and sets its status to _Available_.
  The book is always added to All Books; any libraries ticked in the Libraries field are added as well.
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

### Book Grid

The main **Library** view shows your approved books as a grid of cover thumbnails.

The grid is scoped to the **active library**. Use the library picker in the NavBar to switch
libraries, or click the **Home** button to return to your default library. Selecting **All Books**
shows every book in the catalogue.

- **Sort** books by date added, title, or author using the sort controls
- **Search** using the search bar in the navigation bar — type to filter the displayed books. Prefix terms with `author:`, `title:`, `series:`, `genre:`, or `tag:` for targeted filtering. Search is also scoped to the active library.
- **Filter by shelf** using the shelf pills at the top of the screen
- **Add to library** — drag a book cover onto the **Home** icon in the NavBar to add it to your default library

### Book Detail Page

Click any book to open its detail page. From there you can:

- View full metadata — title, authors, series, description, published year, page count, language, and identifiers (ISBN, ASIN, Hardcover, etc.)
- **Download** the book file — format badges (e.g. `EPUB 2.3 MB`) are download links
- **Edit** metadata — opens the edit page (requires the _Edit Book_ capability)
- **Delete** the book (requires the _Delete Book_ capability)

### Multi-Select & Bulk Operations

Select multiple books from the grid to perform bulk operations:

- **Set reading status** — change the reading status for all selected books at once
- **Edit metadata** — bulk edit a subset of fields (genres, tags, libraries, etc.) across selected books
- **Add to Library** — add all selected books to a library (admin / _Edit Book_ capability;
  a dropdown picker lists non-system libraries). Only shown when custom libraries exist.

Keyboard shortcuts are available for common actions.

---

## Deleting Books & Trash

When you delete a book (from the book detail page or via multi-select), BookBoss removes it
from the database and deletes its library files. Before deletion, the enriched book file
(with metadata and cover art baked in) is automatically copied to a **Trash** directory:

```
{library_path}/Trash/
└── author-title.epub    # enriched copy, ready for re-import
```

This acts as a filesystem safety net — if you change your mind, recovering the book is as
simple as copying the file from Trash back into your Bookdrop directory. The scanner will
pick it up, extract the embedded metadata, and run it through the normal import pipeline.

A few details:

- Only the **enriched** file is copied to Trash (the version with metadata and cover embedded).
  If no enriched file exists, nothing is placed in Trash.
- If a file with the same name already exists in Trash, it is overwritten with the newer version.
- **Rejected** imports do not go to Trash. The original file was moved out of Bookdrop during
  ingestion and rejecting the import deletes it permanently.
- There is no automatic retention policy. Clean up the Trash directory manually when you are
  ready to free the disk space.

---

## Editing Metadata

Click **Edit** on any book's detail page to open the metadata editor. You can update:

- Title, description, language, page count, published year
- Authors (with roles: Author, Editor, Translator, Illustrator)
- Series name and number
- Cover image
- Genres, tags, publishers
- Identifiers (ISBN-13, ISBN-10, ASIN, etc.)
- **Libraries** — which libraries the book belongs to (only shown when custom libraries exist)

Changes are saved to the database and to the OPF sidecar file on disk.

---

## Downloading Books

Format download links appear on each book's detail page under the **Formats** section. Each badge shows the format name and file size. Clicking downloads an enriched copy of the book with up-to-date metadata and cover art embedded.

---

## MOBI Conversion (for Kindle)

BookBoss can generate a **MOBI** file alongside each EPUB so Kindle devices can read books
directly. MOBI conversion is **off by default** and is enabled by an administrator.

### Enabling MOBI Conversion

1. Open **Settings → Application Settings** (admin-only).
2. Toggle **Generate MOBI files for Kindle** on.
3. When enabling, BookBoss prompts whether to backfill MOBI files for books already in the
   library. Choose **Yes** to queue conversion for all existing books, or **No** to only
   generate MOBI files for newly imported books.

When the toggle is on, MOBI generation runs as part of the post-enrichment pipeline — every
book gets a `.mobi` file generated after its EPUB enrichment completes. The MOBI file appears
as an additional download badge on the book detail page.

### Behaviour

- Existing MOBI files stay up to date regardless of the toggle. If a book's metadata changes,
  the MOBI is re-generated even when the toggle is currently off — disabling the toggle only
  stops _new_ MOBI files from being created.
- Backfill runs in the background through the same job queue as imports. Progress shows on the
  job queue badge in the navigation bar.
- MOBI is a one-way derivative of the EPUB. Source-of-truth metadata is still stored on the
  EPUB and OPF sidecar.
