---
slug: feature-koreader-sync
type: feature
status: needs-triage
description: Implement KOReader sync protocol so PocketBook and other KOReader devices can sync reading progress with BookBoss
priority: P2
---

# KOReader Reading Sync

Support the KOReader sync protocol so PocketBook and other e-readers running KOReader can sync reading progress seamlessly with BookBoss.

Source of truth for the protocol: [koreader-sync-server](https://github.com/koreader/koreader-sync-server)

## Requirements

- Implement the 5 KOReader sync endpoints directly in BookBoss (no proxy to external server)
- Mount under `/koreader/v1/` on the existing axum server (port 8080)
- Map KOReader document digests to BookBoss books via filename matching mode (recommended) with binary mode fallback
- Authenticate KOReader users against BookBoss credentials
- Store reading progress in existing `UserBookMetadata` model

## Key Decisions

- **Architecture**: Mount on existing axum server under `/koreader/v1/` — 5 endpoints is too simple to warrant a separate subsystem
- **Document hash**: Recommend filename matching mode (`md5(basename)`) — stable across re-enrichment. Also store hash on first push for binary mode fallback.
- **Auth**: Use BookBoss credentials; `x-auth-key` = `md5(password)` stored/derived alongside the main password hash. Registration endpoint always returns 402 (disabled).
- **Endpoint namespace**: `/koreader/v1/...` — separate from BookBoss's own `/api/v1/...`

## API Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `POST` | `/users/create` | None | Always returns 402 (registration disabled) |
| `GET` | `/users/auth` | Yes | Verify credentials |
| `PUT` | `/syncs/progress` | Yes | Push reading progress |
| `GET` | `/syncs/progress/:document` | Yes | Pull reading progress |
| `GET` | `/healthcheck` | None | Returns `{"state": "OK"}` |

Auth headers: `x-auth-user: <username>`, `x-auth-key: <md5(password)>`

## Integration Surface

`UserBookMetadata` already has compatible fields:
- `progress_percentage` — basis points (0–10000), maps to KOReader percentage × 10000
- `position_type` / `position_token` — store XPointer string or page number
- `last_progress_at` — maps to KOReader timestamp

**Missing**: document digest → `BookId` mapping table (new DB migration required)

## Open Questions / Plan Needed

- How to store the MD5 password hash for KOSync auth alongside the existing strong hash (argon2/bcrypt)?
  - Option A: store `md5(password)` in the users table (weaker, but simple)
  - Option B: derive at verification time from the existing hash (not possible — hashing is one-way)
  - Option C: store `bcrypt(md5(password))` as a separate column
- Schema for the document digest → book mapping table
- Where does the KOReader route handler live? Frontend crate (alongside OPDS) is the natural home
- Full step-by-step implementation plan
