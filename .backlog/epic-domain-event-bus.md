---
slug: epic-domain-event-bus
type: epic
status: needs-triage
description: Introduce a domain event bus in core and redistribute service responsibilities (BookService gains mutations, PipelineService thins down)
priority: P2
---

# Domain Event Bus & Service Responsibility Refactor

`PipelineService` currently owns too much: book CRUD (via `approve_job` and `edit_book`), filesystem ops (renames, sidecar writes, cover storage), and enrichment queueing — in addition to its actual pipeline work. This epic introduces a domain event bus in core and redistributes responsibilities so each service owns its proper domain.

## Requirements

- `BookService` gains mutation methods: `create_book`, `edit_book` (pure DB/domain CRUD)
- `PipelineService` thins down to pipeline orchestration — delegates book mutations to `BookService`
- Domain event bus in core enables reactive workflows (sidecar rewrite, file rename, enrichment)
- Eliminate ~600 lines of duplicated DB mutation code between `approve_job` and `edit_book`
- `LibraryService` retains `delete_book` orchestration (DB + filesystem, no event needed)

## Key Decisions

- **Domain events vs SSE events**: Separate systems. Domain events are internal plumbing; SSE events are an adapter concern. A bridge handler maps domain events → `AppEvent` SSE notifications.
- **`delete_book`**: Stays synchronous. Delete is destructive and users expect it to be atomic.
- **Handler execution**: Fire-and-forget (`tokio::spawn`) — consistent with how enrichment/kepub conversion already work.
- **Subscription model**: Per-event-type registry dispatch. Handlers register for specific event types only (not a broadcast to all).
- **Event payloads**: Individual structs (not enum variants) for type-safe handler registration via `TypeId`.

## Proposed Service Responsibilities

| Method                                          | Current owner            | New owner                                                                     |
| ----------------------------------------------- | ------------------------ | ----------------------------------------------------------------------------- |
| `create_book` (DB transaction from approve_job) | PipelineService          | BookService                                                                   |
| `edit_book` (DB transaction)                    | PipelineService          | BookService                                                                   |
| `delete_book`                                   | LibraryService           | LibraryService (unchanged)                                                    |
| `approve_job`                                   | PipelineService          | PipelineService (thins to: validate → BookService → flip status → emit event) |
| File rename / sidecar rewrite / enrichment      | PipelineService (inline) | Domain event handler                                                          |

## Phase Plan

### Phase 1: Preparatory Refactoring (no event bus needed)

- [x] Rename `LibraryStore` → `FileStoreService`, `LocalLibraryStore` → `LocalFileStore` _(done)_
- [ ] Extract shared DB mutation logic from `approve_job` and `edit_book` into `BookService::edit_book`
- [ ] Extract book creation logic from `approve_job` into `BookService::create_book`
- [ ] `PipelineService` calls `BookService` for mutations, keeps pipeline-specific logic
- [ ] Eliminate duplication between `approve_job` and `edit_book`

### Phase 2: Domain Event Bus

- [ ] Define `DomainEventPayload` marker trait and `DomainEventHandler<E>` trait in `crates/core/src/event/`
- [ ] Define event payload structs: `BookAdded`, `BookChanged`
- [ ] Define `DomainEventBus` port trait (emit + subscribe)
- [ ] Implement `InProcessEventBus` adapter (`HashMap<TypeId, Vec<ErasedHandler>>`, `tokio::spawn` dispatch)
- [ ] Wire `DomainEventBus` into `CoreServices`
- [ ] Register event handlers via `before_start()` calls
- [ ] Add bridge handler: `BookChanged` / `BookAdded` → `EventService::notify_incoming_changed()`

### Phase 3: Migrate Reactive Work to Event Handlers

- [ ] Create handler for `BookChanged`: cover storage, file rename, sidecar rewrite, enrichment
- [ ] `BookService::edit_book` emits `BookChanged` after DB commit
- [ ] `PipelineService::approve_job` uses `BookService` + emits `BookApproved` / `BookChanged`
- [ ] Remove inline file/sidecar/enrichment logic from `PipelineService`

### Phase 4: Future Extensions

- [ ] `BookDeleted` event (if `delete_book` ever moves to event-driven)
- [ ] Additional subscribers (OPDS cache invalidation, audit log)

## Triage Needed

- Break phases into child issues with appropriate `depends-on` chains
- Confirm Phase 1 scope: should the DB mutation extraction be its own PR before the event bus work?
- Decide if Phase 1 and Phase 2 are separate epics or sequential child issues of this epic
