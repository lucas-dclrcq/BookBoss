# OPDS Catalog

BookBoss includes a built-in OPDS 1.x catalog server, allowing you to browse and download books
from any OPDS-compatible reader application.

## Setup

1. Go to your **Profile** page
2. Your OPDS password is auto-generated — copy it, or click **Regenerate** to create a new one
3. In your reader app, add a new OPDS catalog with:
   - **URL:** `http://<your-bookboss-host>:<port>/opds/`
   - **Username:** your BookBoss username
   - **Password:** your OPDS password (not your login password)

   If your reader app does not have separate username/password fields, you can embed the
   credentials in the URL: `http://username:password@<your-bookboss-host>:<port>/opds/`

> **Note:** OPDS uses a separate password from your regular BookBoss login. This is by design —
> OPDS uses HTTP Basic Auth, which transmits credentials with every request.

## Compatible Apps

Any OPDS 1.x compatible reader should work, including:

- KOReader
- Librera Reader
- Moon+ Reader
- Aldiko
- FBReader
- Calibre

## Available Feeds

The OPDS catalog provides the following navigation:

| Feed | URL | Description |
| --- | --- | --- |
| Root catalog | `/opds/` | Entry point with links to all feeds |
| All books | `/opds/all` | Complete library listing |
| Search | `/opds/search?q=...` | Full-text book search |
| Shelves | `/opds/shelves` | Browse by shelf |
| Authors | `/opds/authors` | Browse by author |
| Series | `/opds/series` | Browse by series |

Each book entry includes download links for all available formats and cover images.

## Capabilities

OPDS access requires the **OPDS Access** capability. Administrators can grant this to users from
the user management settings.
