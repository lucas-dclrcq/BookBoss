---
slug: feature-bookdrop-duplicate-removal
type: feature
status: complete
description: Notify user and remove duplicate files from bookdrop instead of silently skipping them forever
priority: P2
---

## Problem

When the bookdrop scanner finds a file whose hash matches an existing `book_files` record or an
active `import_jobs` record, `queue_file_if_new` returns `Ok(())` silently. The file is never
removed from the bookdrop directory, so every subsequent scan re-hashes it, logs a debug-level
skip, and discards it again — indefinitely.

## Desired Behaviour

- Duplicate of a library book → post system message:
  `"{filename}" is already in your library – {author} / {title}. Removed from bookdrop.`
  Then delete the file from bookdrop.
- Duplicate of an incoming-queue entry → post system message:
  `"{filename}" is already in the Incoming Review list. Removed from bookdrop.`
  Then delete the file from bookdrop.

## Technical Approach

1. Add a new enum to `crates/core/src/import/service.rs` (public, alongside the trait):

   ```rust
   pub enum FileQueueStatus {
       Queued,
       DuplicateLibraryFile { title: String, author: String },
       DuplicateIncomingQueue,
   }
   ```

2. Change `queue_file_if_new` signature in both the trait and `ImportJobServiceImpl`:
   `Result<(), Error>` → `Result<FileQueueStatus, Error>`

3. For the `book_files` hit: within the same transaction, call
   `book_repository.find_by_id(tx, book_file.book_id)` for the title and
   `book_repository.authors_for_book(tx, book_file.book_id)` for the first author name.
   Return `DuplicateLibraryFile { title, author }`.

4. For the `import_jobs` hit: return `DuplicateIncomingQueue`.

5. Add `Arc<dyn SystemMessageService>` to `ScanWorker` and thread it through
   `create_bookdrop_scan_subsystem` (new parameter).

6. In `ScanWorker::process_file`, match on the returned status:
   - `DuplicateLibraryFile` / `DuplicateIncomingQueue`: format and post the system message via
     `system_message_service.add_message(...)`, then call `tokio::fs::remove_file(path)`.
   - `Queued`: no change to current behaviour.

7. Wire `system_message_service` from `CoreServices` in `bookboss/main.rs` (or wherever
   `create_bookdrop_scan_subsystem` is called).

8. Update `MockImportJobService` (re-run mockall derive / update hand-written mock) and all
   affected unit tests for the new return type. Add component tests covering both duplicate paths,
   verifying message content and that `add_message` is called exactly once.

## Acceptance Criteria

- Duplicate library file: system message appears in the Messages UI, file is removed from
  bookdrop, never re-scanned on subsequent ticks.
- Duplicate incoming file: same.
- New unique file: unchanged behaviour — queued, file remains in bookdrop until the pipeline
  moves it.
- Component tests cover both duplicate paths including expected message text.
- No regression in existing `queue_file_if_new` tests.
