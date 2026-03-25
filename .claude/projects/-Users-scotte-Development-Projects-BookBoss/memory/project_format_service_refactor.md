---
name: FormatService refactor
description: Planned refactor to introduce FormatService as sole interface to formats crate — replaces MetadataExtractor + ConversionService traits, moves job handlers to core
type: project
---

FormatService refactor planned and approved (2026-03-24). Scratchpad: `.scratchpad/feature-format-service.md`.

**Why:** Format concerns are scattered across MetadataExtractor, ConversionService, direct bb_formats imports,
and job handlers that reach into DB. Consolidating behind FormatService means adding a new format only requires
changes inside the formats crate.

**Key decisions:**

- `FormatService` trait in new `core/format/` domain — `extract_metadata(path)`, `enrich(request)`, `write_sidecar(path)`, `read_sidecar(path)`, `detect_format(path)`
- `EBookFile` = `(FileFormat, PathBuf)` — used for source and outputs in `EnrichmentRequest`
- Formats crate becomes pure file I/O — no DB, no jobs, no CoreServices
- Job handler moves to core `format/` domain — loads DB data, calls FormatService, persists results
- Single job type `"enrich_book_files"` replaces `"enrich_epub"` + `"convert_kepub"`
- Hashing is caller's responsibility (FileStoreService), not FormatService
- Scanner format detection deferred to import crate rework
- CLI `dump-epub` → `dump-book` using FormatService::extract_metadata
- FormatServiceImpl is stateless (unit struct)
- Each phase is a separate jj changeset
