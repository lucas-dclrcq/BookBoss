---
slug: feature-replace-cover
type: feature
status: complete
description: Allow replacing a book's cover by dragging an image file onto the cover in the review/edit metadata screens
priority: P2
---

# Replace Cover via Drag-and-Drop

While reviewing an incoming book or editing a book's metadata, the user should be able to drag a JPG or PNG image file from their computer onto the current cover picture to replace it.

## Requirements

- Drag-and-drop an image onto the cover replaces it
- Available on both the incoming book review screen and the edit metadata screen
- Accept JPG, PNG, WebP, GIF — all normalized to JPEG on storage (see below)
- 10 MB client-side size guard before encoding
- Optimistic UI: preview updates immediately on drop; reverts on server error
- Capability checks stay in the server fn layer (consistent with existing pattern)

## Design Decisions

- **All covers normalized to JPEG** regardless of source (user upload or provider fetch). Kobo firmware is most reliable with JPEG. Normalization: resize to fit within 1024×1536 if needed (skip resize if already within bounds), re-encode at Q85 (strips EXIF/XMP/IPTC), always output `cover.jpg`.
- **`replace_cover` is immediate** — saves directly to the book's directory on drop confirmation, no temp dir involvement. Both candidate (review) and library books have a `BookToken` and a cover at `{library_path}/{book_token}/cover.jpg`, so one method serves both screens.
- **Two server fns** following the existing split pattern (`fetch_provider_metadata` vs `fetch_provider_for_edit`): one checks `ApproveImports`, one checks `EditBook`.
- **Upload via base64 string** in server fn arg (no multipart endpoint needed).

## Implementation Plan

### Changeset 1 — Normalize all covers to JPEG in storage

**`crates/storage/src/local.rs`**

- Rename `normalize_jpeg` → `normalize_to_jpeg` to signal it accepts any format as input
- In `store_cover`, detect input format via magic bytes (same logic as `detect_cover_filename`):
  - JPEG → `normalize_to_jpeg` (strips metadata + conditional resize)
  - PNG / WebP / GIF → `normalize_to_jpeg` (converts via `with_guessed_format()` decode + JPEG re-encode; resize only if needed)
  - Unknown/corrupt → fall back to original bytes
- Always write as `cover.jpg` regardless of input format; remove filename-conditional branch

**`crates/core/src/library/service.rs`** and pipeline call sites

- Simplify `detect_cover_filename` to always return `"cover.jpg"`, or remove it and hardcode at call sites

### Changeset 2 — `LibraryService::replace_cover`

**`crates/core/src/library/service.rs`** — add to trait:

```rust
async fn replace_cover(
    &self,
    book_token: BookToken,
    cover_bytes: Vec<u8>,
) -> Result<(), Error>;
```

Implementation:

1. Find book by token — return error if not found
2. Call `file_store.store_cover(token, "cover.jpg", &cover_bytes)` — normalization is transparent
3. Update `book.cover_path = Some("cover.jpg".to_string())`
4. Rewrite sidecar only for approved library books (skip for review candidates)
5. Save book to DB

**`crates/core/src/test_support.rs`** — add mock impl

### Changeset 3 — Frontend server fns + drag-drop UI

**`crates/frontend/src/routes/review_page/server.rs`** — two server fns:

- `replace_incoming_cover(job_token: String, data_base64: String)` — requires `ApproveImports`; resolves candidate `book_token` from the job, decodes base64, calls `replace_cover`
- `replace_library_cover(book_token: String, data_base64: String)` — requires `EditBook`; decodes base64, calls `replace_cover`

**`crates/frontend/src/routes/review_page/editor.rs`** — drag-drop on the cover `<img>`:

- `ondragover` — prevent default to allow drop; show hover state (dashed border or opacity change)
- `ondrop` — validate `file.type` starts with `image/`; reject if size > 10 MB; create object URL for optimistic preview; call appropriate server fn (`replace_incoming_cover` or `replace_library_cover` based on `edit_mode`); revert preview on error
