---
slug: bug-only-delete-terminal-import-jobs
type: bug
status: complete
description: Scanner dedup misidentifies incoming-queue files as library duplicates; rejected files block re-import
priority: P2
---

# Issues

The original report mentioned four concerns. Two are already implemented correctly:

- `CleanupOldImportJobsHandler` → `delete_old_terminal_jobs` already only deletes `Approved`/`Rejected` jobs.
- The cleanup query already uses `updated_at` (not `created_at`) as the cutoff.

The two real bugs are in `queue_file_if_new` (`crates/core/src/import/service.rs`):

---

## Bug A — `find_file_by_hash` matches Incoming books

**File**: `crates/database/src/adapters/book.rs:276` (`find_file_by_hash`)

The import pipeline stores a `book_files` row for a candidate book as soon as it begins processing — before the book is approved. At that point the book's status is `Incoming`, not `Available`.

`find_file_by_hash` has no join to the `books` table and no status filter, so it matches `book_files` rows linked to `Incoming` books. When the same file is dropped again in the bookdrop, `queue_file_if_new` finds the row, looks up the book (which is still `Incoming`), and returns `DuplicateLibraryFile` — logging "already in your library" when the file is actually still in the incoming review queue.

**Fix**: Add a join to `books` and filter `books.status = 'available'` in `find_file_by_hash`, so it only returns files for fully-approved library books.

---

## Bug B — `find_by_hash` on import_jobs matches terminal statuses

**File**: `crates/database/src/adapters/import_job.rs:128` (`find_by_hash`)

`find_by_hash` has no status filter — it matches **any** import job with that hash, including `Rejected`, `Approved`, and `Error` jobs. A user who rejects a file and then re-drops it in the bookdrop will see `DuplicateIncomingQueue` and the file will be silently removed from the bookdrop without being re-queued.

**Fix**: Filter `find_by_hash` to only match active (non-terminal) statuses: `Pending`, `Extracting`, `Identifying`, `NeedsReview`. Consider renaming to `find_active_by_hash` to make the intent explicit. Update the `ImportJobRepository` trait and mock accordingly.

---

## Acceptance Criteria

- [ ] Dropping a file that is in the incoming queue returns `DuplicateIncomingQueue` (not `DuplicateLibraryFile`)
- [ ] Dropping a file whose import was previously rejected re-queues it successfully
- [ ] Dropping a file already in the library (Approved + Available) still returns `DuplicateLibraryFile`
- [ ] `CleanupOldImportJobsHandler` is unaffected (it already calls `delete_old_terminal_jobs`)
- [ ] Postgres integration tests cover all three dedup scenarios
