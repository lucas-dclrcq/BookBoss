---
slug: bug-bad-log-enrichment-job
type: bug
status: not-started
description: Log message doesn't indicate which file had the issue
priority: P2
---

## Problem

`enrich_book_files` job fails with a generic error log that includes no context about which book or file caused the problem:

```
ERROR bb_core::jobs::worker: job handler failed job_type="enrich_book_files" error=Infrastructure error: XML error: ill-formed document: entity or character reference not closed: `;` not found before end of input
```

The XML error originates in `crates/formats/src/opf/parse.rs` during OPF parsing. The `book_id` is available in the handler payload but is never included in the error path. The job worker only logs `job_type` + error string.

Additionally, some import jobs are stuck in `Extracting` state where the original file path no longer exists on disk. These get reset to `Pending` by `reset_stale_import_jobs`, causing them to fail immediately and loop indefinitely.

## Requirements

Two paths when any job handler or health check detects an error:

1. **Corrective action available** → take the action (e.g., reset to Pending if the file still exists)
2. **No corrective action** → post a system message with enough context for the admin to investigate

The log and system message must include context: book_id, file path, and the error.

## Technical Approach

### Fix 1 — `EnrichBookFilesHandler` (`crates/core/src/format/handler.rs`)

The handler already holds `Arc<CoreServices>` (which includes `system_message_service`). Wrap the handle body to catch errors before they propagate to the worker:

```rust
async fn handle(&self, payload: EnrichBookFilesPayload) -> Result<(), Error> {
    let book_id = payload.book_id;
    let result = self.run(book_id).await;
    if let Err(ref e) = result {
        tracing::error!(book_id, error = %e, "enrich_book_files failed");
        let _ = self.core.system_message_service
            .add_message(NewSystemMessage {
                source_task: "jobs.enrich_book_files".to_owned(),
                severity: MessageSeverity::Error,
                message: format!("Enrichment failed for book {book_id}: {e}"),
            })
            .await;
    }
    result
}

async fn run(&self, book_id: BookId) -> Result<(), Error> {
    // existing handler body moved here
}
```

Once the file path is resolved inside `run()`, add it to the tracing span:
```rust
tracing::Span::current().record("file_path", &file_path_str);
```

And update the top of `handle()` to open an instrumented span:
```rust
let span = tracing::error_span!("enrich_book_files", book_id, file_path = tracing::field::Empty);
let _enter = span.enter();
```

### Fix 2 — `reset_stale_import_jobs` health check (`crates/core/src/health/handlers/reset_stale_import_jobs.rs`)

When a stale `Extracting`/`Identifying` job is found, check whether the file path exists on disk:

- **File exists** → reset to `Pending` as before (corrective action available)
- **File missing** → set status to `Error` with message `"file no longer exists at {path}"`, and post a system message:
  ```
  source_task: "health.reset_stale_import_jobs"
  severity: MessageSeverity::Error
  message: "Import job {token} moved to Error: file no longer exists at {path}"
  ```

The handler already has access to the file path via the `ImportJob` record's `file_path` field. Use `tokio::fs::try_exists` or check via `FileStoreService` if available; otherwise `std::path::Path::new(&job.file_path).exists()` is acceptable here since this is a health check, not a hot path.

## Acceptance Criteria

- [ ] `enrich_book_files` error logs include `book_id` and file path (once resolved) as structured fields
- [ ] When enrichment fails, a system message appears in the Messages UI with book_id and the error text
- [ ] Import jobs stuck in `Extracting`/`Identifying` with a missing file path are moved to `Error`, not `Pending`
- [ ] A system message is posted for each import job moved to `Error` state via the health check
- [ ] Jobs whose file still exists continue to be reset to `Pending` (no regression)
- [ ] Component tests cover: enrichment failure posts exactly one system message with book_id in the text; stale job with missing file → Error + system message; stale job with present file → Pending, no message
