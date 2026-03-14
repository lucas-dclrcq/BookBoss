# BookBoss - Take Control Of Your Digital Library

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0](https://github.com/szinn/BookBoss/compare/v0.2.5..v0.3.0) - 2026-03-14

### Features

- _(core)_ MP.4 + MP.5 — DeviceService trait, impl, and CoreServices wiring - ([e8de684](https://github.com/szinn/BookBoss/commit/e8de68438588ed3ebe19caad8db07ac5db66749e))
- _(core)_ MP.3 — ShelfService::create_device_shelf + find_device_shelf - ([021c5aa](https://github.com/szinn/BookBoss/commit/021c5aae3e34be47cc5c569746ee3784f8fad69d))
- _(core)_ Add filter domain with BookFilter tree model - ([23422e1](https://github.com/szinn/BookBoss/commit/23422e1dddb02c50810e62b4ed9a703b2c6e8de4))
- _(core,database)_ MP.2 — ShelfRepository::find_by_device_id - ([db12232](https://github.com/szinn/BookBoss/commit/db1223247ae9c8c41ff9c970e7dc3f83be3566cb))
- _(core,database)_ MP.1 — DeviceRepository CRUD + database adapter - ([1e9a4bb](https://github.com/szinn/BookBoss/commit/1e9a4bb74f65320d9b0a324a0d16cbaf748fbf61))
- _(core,database)_ Replace ShelfFilter with BookFilter tree (M6.2) - ([f35764f](https://github.com/szinn/BookBoss/commit/f35764f1ec5b7e835d24706bb0657ff810c43c05))
- _(database)_ Implement build_condition() filter translator (M6.3) - ([e40e060](https://github.com/szinn/BookBoss/commit/e40e060a4f2f63f76712b3fc2cc43c3882eba087))
- _(frontend)_ MP.13 — disable shelf delete for device-linked shelves - ([df254b9](https://github.com/szinn/BookBoss/commit/df254b96666a62aca58cd1fe410fb6e8ea404a6a))
- _(frontend)_ MP.12 — My Devices section on Profile page - ([97e3a6f](https://github.com/szinn/BookBoss/commit/97e3a6fe21e262397ee4c44757a2c59d9b0fb961))
- _(frontend)_ MP.11 — move Reading section from Settings to Profile - ([7942131](https://github.com/szinn/BookBoss/commit/79421312bbfa3b16d075247da760da2663bc02ad))
- _(frontend)_ MP.10 — profile section with modal password change - ([7e53ce2](https://github.com/szinn/BookBoss/commit/7e53ce20ff7503c04a87c79aab06da73fa32cc08))
- _(frontend)_ Rework ProfilePage to single-page scroll layout - ([23c31d8](https://github.com/szinn/BookBoss/commit/23c31d83f84be5bc76ee70dfe9d1d70f3f2cca72))
- _(frontend)_ MP.8 + MP.9 — Profile link in NavBar and ProfilePage skeleton - ([a5154b2](https://github.com/szinn/BookBoss/commit/a5154b23ed1bb4bc9e4565b1636ad205105a6349))
- _(frontend)_ M6.7 — badge counts on smart shelf pills - ([dabde74](https://github.com/szinn/BookBoss/commit/dabde74c5e8f33cab3854125878f05405ac5c79a))
- _(frontend)_ ReadStatus filter rule — chip input with centered hint - ([fb8abad](https://github.com/szinn/BookBoss/commit/fb8abad464884f5bca437a31c0761b86af1a0149))
- _(frontend)_ M6.6 — smart shelf detail page - ([26f818c](https://github.com/szinn/BookBoss/commit/26f818cf75135c4dd70fd8e797dc6945ce4d7579))
- _(frontend)_ M6.5 smart shelf create/edit UI - ([a1f1547](https://github.com/szinn/BookBoss/commit/a1f15478a6a0d9b6b602d2208fdc530c77c8ba0e))
- _(frontend)_ M6.4 filter builder component and entity options server fn - ([bcb6072](https://github.com/szinn/BookBoss/commit/bcb60724e9cc471d255f27555709148eaec4377f))

### Bug Fixes

- _(core)_ Sync device name when companion shelf is renamed - ([3f406a1](https://github.com/szinn/BookBoss/commit/3f406a10c4f899c7044c094013b83e9b5baa841f))
- _(frontend)_ Resize graphic assets - ([ee46937](https://github.com/szinn/BookBoss/commit/ee46937267448c56c7c14af407a060f94f295493))

### Refactor

- _(core)_ Clippy warning cleanup and lint tuning - ([242a3c7](https://github.com/szinn/BookBoss/commit/242a3c73df0d17f639be1bbdc8a45321f324abad))
- _(core,database,frontend)_ Rename BookFilter → BookQuery in book domain - ([848feb6](https://github.com/szinn/BookBoss/commit/848feb6247d3d371d8e14e0812ee20fdf283d73f))

### Miscellaneous Tasks

- _(bookboss)_ Add mimalloc as global allocator - ([a9452d3](https://github.com/szinn/BookBoss/commit/a9452d3f549e58122f1ae8114beb87ef1d895ef8))
- _(core)_ Suppress remaining cast warnings with #[expect] and reason - ([fae77bf](https://github.com/szinn/BookBoss/commit/fae77bf87ab90930bd7e974eeac34414c03247b4))
- _(core)_ Remove clone_on_ref_ptr lint; allow SeaORM cast warnings - ([eaf8265](https://github.com/szinn/BookBoss/commit/eaf8265f732cc5689bcb7d53d5c15c3db885dbe8))

## [0.2.5](https://github.com/szinn/BookBoss/compare/v0.2.4..v0.2.5) - 2026-03-11

### Features

- _(frontend)_ Keep NavBar stable during route navigation - ([a4e8658](https://github.com/szinn/BookBoss/commit/a4e8658bcb0f428a07f50f430740ddc926a7415b))
- _(frontend)_ Add /healthz and /readyz health endpoints - ([a22c7a2](https://github.com/szinn/BookBoss/commit/a22c7a2309732377c93535eac53ab21fc591409c))

### Bug Fixes

- _(frontend)_ Limit inbound HTTP request body size to 10 MiB - ([1db685f](https://github.com/szinn/BookBoss/commit/1db685f0a0f69b5fcbb05ba99ba5e3f0e60021a1))
- _(frontend)_ Fix banner image width on mobile screens - ([6272ca2](https://github.com/szinn/BookBoss/commit/6272ca2be93455ffbeb5ab046305e47b83b63565))
- _(metadata)_ Add 5s request timeout to all metadata providers - ([a90ada8](https://github.com/szinn/BookBoss/commit/a90ada8c55384245f760caa46b43010abf927b21))

### Miscellaneous Tasks

- _(clippy)_ Clean up some clippy warnings - ([3aa0b3f](https://github.com/szinn/BookBoss/commit/3aa0b3f2b7015275593da793d25ed57aa61105a3))

## [0.2.4](https://github.com/szinn/BookBoss/compare/v0.2.3..v0.2.4) - 2026-03-11

### Features

- _(frontend)_ Display genres and tags on book detail page - ([b598ddf](https://github.com/szinn/BookBoss/commit/b598ddf71add84a139820280ca34b91aae79cb0d))

### Bug Fixes

- _(frontend)_ Tie banner graphic to login/register widths - ([8bbf01a](https://github.com/szinn/BookBoss/commit/8bbf01ab9fa2f1f9defe5f1ae64b33e1c12686fd))
- _(frontend)_ Capability-gate edit/delete buttons and add delete confirmation - ([d22d8b5](https://github.com/szinn/BookBoss/commit/d22d8b5cf14c048bcb69631e7627fed43f256f4a))
- _(frontend)_ Programmatically focus username field on login form mount - ([2171e38](https://github.com/szinn/BookBoss/commit/2171e3834bb746e6241b8cea9f52278ab971c6eb))

### Refactor

- _(frontend)_ Integrate frontend as tokio-graceful-shutdown subsystem - ([8487aec](https://github.com/szinn/BookBoss/commit/8487aec95a86972073bad8df27d1f24670073db3))

## [0.2.3](https://github.com/szinn/BookBoss/compare/v0.2.2..v0.2.3) - 2026-03-09

### Features

- _(core,database,frontend)_ Add full_name and change_password_on_login to user model - ([a7f1bdf](https://github.com/szinn/BookBoss/commit/a7f1bdfa0cbc8d975a2362dda453467601abc0d1))
- _(core,frontend)_ Add user management settings tab - ([27e2112](https://github.com/szinn/BookBoss/commit/27e2112c52184456454b8432504a4cd94919f352))
- _(core,frontend)_ Add clear rating button on book detail page - ([1d687a2](https://github.com/szinn/BookBoss/commit/1d687a277b8bdc44acd68a821337a75c344d4b85))
- _(frontend)_ User management, force password change, and polish - ([6782866](https://github.com/szinn/BookBoss/commit/67828667cf862cbedea502b0f15ba0171467dd58))

### Bug Fixes

- _(frontend)_ Restart shelf book resource when navigating between shelves - ([bb146e5](https://github.com/szinn/BookBoss/commit/bb146e57de80d12ba7d2f0bcc0cb753aac829cc3))

## [0.2.2](https://github.com/szinn/BookBoss/compare/v0.2.1..v0.2.2) - 2026-03-09

### Features

- _(core,frontend)_ Replace Dnf with Paused/Rereading/Abandoned reading statuses - ([8a2adb5](https://github.com/szinn/BookBoss/commit/8a2adb5d30e821f097d684f82b609dc5bdbb382b))

### Refactor

- _(frontend)_ Trim book detail reading panel and reorder sections - ([626e779](https://github.com/szinn/BookBoss/commit/626e77973d49eec17a8aaf70adaee17a6d8adb9c))

## [0.2.1](https://github.com/szinn/BookBoss/compare/v0.2.0..v0.2.1) - 2026-03-09

### Features

- _(core)_ Add ReadingService with state machine - ([7c4ac3d](https://github.com/szinn/BookBoss/commit/7c4ac3ddf30bf4f2128567952daca8d66cab68ee))
- _(database)_ Add UserBookMetadataRepository adapter - ([4fa283f](https://github.com/szinn/BookBoss/commit/4fa283f682b6a7d7fcced9dc768a5270a1035f25))
- _(frontend)_ Add Currently Reading section to books page - ([565a677](https://github.com/szinn/BookBoss/commit/565a677492ba989eb2d7262de256256f97f8f643))
- _(frontend)_ Add reading status badges to book cards - ([03270a8](https://github.com/szinn/BookBoss/commit/03270a831aa581be0c858f83ce2929f04e62ac85))
- _(frontend)_ Add reading controls to book detail page - ([41edaf6](https://github.com/szinn/BookBoss/commit/41edaf6523e9febe4927e02b1a5c487da852daee))
- _(frontend)_ Add auto-read threshold setting - ([aa3d1c0](https://github.com/szinn/BookBoss/commit/aa3d1c0ceee41447e22ba3e804ed857b0c086563))

### Refactor

- _(core)_ Sort capabilities - ([e31ae14](https://github.com/szinn/BookBoss/commit/e31ae149ff2cb4c18b94b64cb8794c67ef0bcabb))
- _(frontend)_ Match shelf bar and Currently Reading backgrounds to book grid - ([c570a5b](https://github.com/szinn/BookBoss/commit/c570a5b575707ff7813bdeb87329e9140454839d))

### Testing

- _(integration)_ Added MariaDB integration tests - ([a2c87eb](https://github.com/szinn/BookBoss/commit/a2c87ebf14f4af388444e0238150abb3ab2691f6))

## [0.2.0](https://github.com/szinn/BookBoss/compare/v0.1.16..v0.2.0) - 2026-03-08

### Features

- _(core)_ Add ShelfService trait and ShelfServiceImpl - ([762c6ec](https://github.com/szinn/BookBoss/commit/762c6ec7f9f38808448e54bcbfdca651da6dca6d))
- _(core,database,frontend)_ Add genre/tag fields and picklist data structures - ([e450235](https://github.com/szinn/BookBoss/commit/e4502359fb309ac7d249a7e5a0d102d003cc074c))
- _(database)_ Implement ShelfRepository adapter (M4.1) - ([d5a3e8c](https://github.com/szinn/BookBoss/commit/d5a3e8c2c22cb764490930e88b339de257746261))
- _(frontend)_ Add book file download endpoint and format download buttons - ([3df59c5](https://github.com/szinn/BookBoss/commit/3df59c5779ee3a94c0f7594c6de6cbad98abbeb0))
- _(frontend)_ Remove grid/table toggle button from NavBar - ([db0f1b9](https://github.com/szinn/BookBoss/commit/db0f1b98e3085756c358e10527a931858d77251e))
- _(frontend)_ Add DnD success flash, rename Edit buttons (M4.9) - ([3887d06](https://github.com/szinn/BookBoss/commit/3887d06432c1ef30d3c397c5b23f331d8389ebca))
- _(frontend)_ Drag-and-drop books onto shelf pills (D.1–D.3) - ([b635ecd](https://github.com/szinn/BookBoss/commit/b635ecde691341c147e5259bcac75df599101e49))
- _(frontend)_ Unified ShelfBar+BookGrid UX redesign (C.1–C.6) - ([36c179a](https://github.com/szinn/BookBoss/commit/36c179a15baed59c19a95f738762e13dd43a4e63))
- _(frontend)_ Add new shelf modal with name validation - ([a9cfbfa](https://github.com/szinn/BookBoss/commit/a9cfbfa3df0045d2231cd36516b3da9ea8d20334))
- _(frontend)_ Add shelves sidebar bar and ShelfPage route (M4.6) - ([d42c25a](https://github.com/szinn/BookBoss/commit/d42c25ad3e47a5402de99f7e11ef4294aa3676fa))
- _(frontend)_ Add books_for_shelf server function (M4.5) - ([095104d](https://github.com/szinn/BookBoss/commit/095104d71276f9a7aa6438275ba9792377d0c635))
- _(frontend)_ Add shelf CRUD server functions (M4.4) - ([0d8bb8c](https://github.com/szinn/BookBoss/commit/0d8bb8caea45e2345526ff7a7272a283b8dac5b0))
- _(frontend)_ Add ChipInput/AutocompleteInput components for picklist support - ([4f175c0](https://github.com/szinn/BookBoss/commit/4f175c063cf78f7f9fc8d0fcfbb7777c3dfb5364))
- _(frontend,core)_ UX redesign prep — update_shelf, BookSummary unification (A.1, A.2, B.1, B.3) - ([86adfd8](https://github.com/szinn/BookBoss/commit/86adfd8ec23b2b7776e24d9d0fef618d0f5bc7e6))
- _(metadata)_ Add Google Books metadata provider - ([ba57ce3](https://github.com/szinn/BookBoss/commit/ba57ce3493503d6bcb52918d2b40bed86afe300d))

### Bug Fixes

- _(database)_ Create jobs_claim index via SeaORM DSL instead of raw SQL - ([61d17f6](https://github.com/szinn/BookBoss/commit/61d17f64bbd4fbb67860f58943c3e6ea156663c3))
- _(database)_ Use regular index for jobs_claim on MySQL - ([c32c246](https://github.com/szinn/BookBoss/commit/c32c2468cf01779cbb5ce7ad22198e94d9cfaff0))
- _(frontend)_ Add 'All Books' entry to Shelves section in table view explorer - ([9cbae86](https://github.com/szinn/BookBoss/commit/9cbae868183964a5148b01c667dbe93368c28fe6))
- _(frontend)_ Change books_for_shelf server fn from GET to POST - ([d964ef0](https://github.com/szinn/BookBoss/commit/d964ef09977c47a901de87e259b14d40bd8d729c))
- _(frontend)_ Fix auth gaps in server functions - ([2e17165](https://github.com/szinn/BookBoss/commit/2e17165235a4e4d2ef923855b6c38c92eae23631))

### Refactor

- _(frontend)_ Split review_page into module with types/server/editor submodules - ([a47feee](https://github.com/szinn/BookBoss/commit/a47feee5b2d166e20037d563cb6a53c87c35f78c))

### Documentation

- Update README and docs to reflect current feature set - ([2c1879c](https://github.com/szinn/BookBoss/commit/2c1879cdffdee6cc76b2e2bff094b91ac977baff))

### Testing

- _(core)_ Add component tests for ShelfService (M4.3) - ([0a41188](https://github.com/szinn/BookBoss/commit/0a41188e44a78753cf9ab273fcc2ff7d856b07ab))
- _(integration)_ Add integration tests for book, import_job, and library services - ([a5e4584](https://github.com/szinn/BookBoss/commit/a5e4584e1ae28f068630eeee243ce0961a7405db))

### Miscellaneous Tasks

- _(database)_ Add missing indicies - ([fde9d54](https://github.com/szinn/BookBoss/commit/fde9d544792c1e29e47a435541c6339c866650ce))

## [0.1.15](https://github.com/szinn/BookBoss/compare/v0.1.14..v0.1.15) - 2026-03-07

### Features

- _(core,frontend)_ Add LibraryService with stats and delete book - ([0a2152e](https://github.com/szinn/BookBoss/commit/0a2152e8a8e1289eb67249d29890869967d80fef))
- _(core,frontend)_ Edit metadata page for library books - ([ae1425a](https://github.com/szinn/BookBoss/commit/ae1425a41f91999a6246a8e021f34799558ba949))
- _(import)_ Separate scan and worker poll intervals - ([6261611](https://github.com/szinn/BookBoss/commit/62616111a1b8b16eb5012c2eee2ff538b5413a75))

### Bug Fixes

- _(frontend)_ Refresh NavBar pending count after approve/reject - ([5f086fe](https://github.com/szinn/BookBoss/commit/5f086fefb7aa5f8c4935d3a5e445619f42dcbc3c))

### Refactor

- _(core)_ Decouple fetch_from_provider from ImportJobToken - ([acdd94d](https://github.com/szinn/BookBoss/commit/acdd94dc9b810be00aa55d31c8b9cb8bc64e2855))

### Documentation

- _(claude)_ Restructure CLAUDE.md files for clarity and reuse - ([2869eb3](https://github.com/szinn/BookBoss/commit/2869eb3bc99ca4a0c2fc65eac5cc8e72c2f303a7))
- _(user)_ Update import configuration reference - ([2746432](https://github.com/szinn/BookBoss/commit/27464325887dd9aeab0e03aac0692455444650fe))

## [0.1.14](https://github.com/szinn/BookBoss/compare/v0.1.12..v0.1.14) - 2026-03-07

### Features

- _(bookboss)_ Wire scanner and worker subsystems into server startup - ([c919c74](https://github.com/szinn/BookBoss/commit/c919c741338360fab67117d8aa8cd0d369cb3516))
- _(core)_ Add ApproveImports capability - ([6b4c2a2](https://github.com/szinn/BookBoss/commit/6b4c2a2a3203877cdeeda22573d7e37a692c8717))
- _(core)_ Independent metadata and cover selection across providers - ([8c11159](https://github.com/szinn/BookBoss/commit/8c111596552513dd7a086ebde84681558e6f6a25))
- _(core)_ Include first author in stored book file slug - ([9ef8b0b](https://github.com/szinn/BookBoss/commit/9ef8b0b39b21280a6ad08e6d83e3d2562ea08f2c))
- _(core,database,frontend)_ Book review and approval page (M3.22) - ([e0a4065](https://github.com/szinn/BookBoss/commit/e0a406535550e772af0c979e1a19ad7d348965a7))
- _(core,database,frontend)_ Incoming books list with reject cleanup - ([e0416a8](https://github.com/szinn/BookBoss/commit/e0416a838e295a7582d3f27a863b8d66f88de288))
- _(core,frontend)_ Best-resolution cover selection; review page dimensions - ([20b99d4](https://github.com/szinn/BookBoss/commit/20b99d4afe4753d226969c267f2e767cfb6bf2eb))
- _(core,frontend)_ Provider search uses current form identifiers - ([db6b936](https://github.com/szinn/BookBoss/commit/db6b936515e82c3003a43df477bb11a0ee34e3a8))
- _(database)_ Case-insensitive find_by_name across entity adapters - ([b7d22f8](https://github.com/szinn/BookBoss/commit/b7d22f8ae60fae0e73e2dfc7905eadbd203147c1))
- _(database)_ Add JobRepositoryAdapter with optimistic-locking claim loop - ([d6ec64a](https://github.com/szinn/BookBoss/commit/d6ec64a419f73cee44cd1f47a2b6dccc9cb0d92e))
- _(formats,core)_ Extract cover image from EPUB file - ([3cbf48c](https://github.com/szinn/BookBoss/commit/3cbf48cdd9141eda19dd503f678d9f684d278993))
- _(frontend)_ Simplify cover serving — token-only endpoint with blank fallback - ([996e565](https://github.com/szinn/BookBoss/commit/996e5658201ddf1c90631933a92aba1dc35d0b79))
- _(frontend,import)_ Pending count badge on Incoming nav link; fix tracing field - ([4b3e40e](https://github.com/szinn/BookBoss/commit/4b3e40ede88006fbe7aedfacea9add489171e72b))
- _(import)_ Add bb-import crate with scanner subsystem and process_import handler - ([8b0f9b4](https://github.com/szinn/BookBoss/commit/8b0f9b454bc2fa98dd8683696ec3adb1e5aa8e08))

### Bug Fixes

- _(api)_ Enable TLS for gRPC client connections - ([75a660c](https://github.com/szinn/BookBoss/commit/75a660c85189a0b84960243475b8617e72b4f440))
- _(import,core)_ Pipeline robustness and startup recovery - ([30d4825](https://github.com/szinn/BookBoss/commit/30d48251d2c1080b36346d64f645e2db0cb5e8c5))

### Documentation

- Add storage crate and M3 config vars to README and docs - ([84e56ea](https://github.com/szinn/BookBoss/commit/84e56ea158d1a29a78061df9c6a64d955a82fe19))

### Miscellaneous Tasks

- _(core,import)_ Improve subsystem and pipeline tracing - ([6396e4d](https://github.com/szinn/BookBoss/commit/6396e4d18d5649a886bf72289ebf642478e2bab8))

## [0.1.12](https://github.com/szinn/BookBoss/compare/v0.1.11..v0.1.12) - 2026-03-06

### Features

- _(bookboss)_ Add ImportConfig with watch_directory and poll_interval_secs - ([fe22822](https://github.com/szinn/BookBoss/commit/fe228229e2a9e2a34630189d5867f3d3096e238b))
- _(core)_ Add job handler registry and worker subsystem - ([060c6b3](https://github.com/szinn/BookBoss/commit/060c6b3db06bd8b9112313f4b817214f378d1753))
- _(core,database)_ Add job queue port traits and wire into RepositoryService - ([7d11484](https://github.com/szinn/BookBoss/commit/7d11484ada1f8559facc1189c8bedd36215d03b9))
- _(database)_ Add jobs table migration and entity for job queue - ([d538345](https://github.com/szinn/BookBoss/commit/d538345a9ad104b193a491636749fd072eab035d))
- _(grpc)_ Add reflection API - ([c982937](https://github.com/szinn/BookBoss/commit/c982937efdcbe985d39ead0f7b45bce9a3d0e1b6))

## [0.1.11](https://github.com/szinn/BookBoss/compare/v0.1.10..v0.1.11) - 2026-03-05

### Features

- _(api,cli)_ Add grpc CLI command and fix system status endpoint - ([38d13c2](https://github.com/szinn/BookBoss/commit/38d13c2d54c533937cb442e101ea50539622cf42))
- _(cli)_ Add dump-epub command for exploring EPUB metadata - ([7c42900](https://github.com/szinn/BookBoss/commit/7c42900ae069357c3a9a0a11f70cd6adf267f0c5))
- _(core)_ Add PipelineService for M3.11 acquisition pipeline - ([85dae12](https://github.com/szinn/BookBoss/commit/85dae125f5445a4d77aa4fb5682d37f6e8bf15f1))
- _(formats)_ Add read_opf_metadata_xml helper for diagnostics - ([4f24509](https://github.com/szinn/BookBoss/commit/4f2450996f83b2614a9e669f61f405528a305ed6))
- _(formats)_ EPUB metadata extraction - ([af39d9f](https://github.com/szinn/BookBoss/commit/af39d9fb99f38714a2a673fe3d4f7b553557d4b4))
- _(formats)_ OPF sidecar parse and write - ([0e08af3](https://github.com/szinn/BookBoss/commit/0e08af3e329595d7e5b476238b46581f7f223f31))
- _(metadata)_ Add openlibrary CLI command and Default impl - ([c02f304](https://github.com/szinn/BookBoss/commit/c02f304e5811ea0a30dbe7606ae091a2224282c6))
- _(metadata,cli)_ Add HardcoverAdapter MetadataProvider and CLI command - ([edd6b5b](https://github.com/szinn/BookBoss/commit/edd6b5b42cc1bb5336dc043d695f259da629ec00))
- _(storage)_ Implement LocalLibraryStore for local filesystem storage - ([d7c7f7e](https://github.com/szinn/BookBoss/commit/d7c7f7e2717dd06dca49f5373131440028b9f574))
- _(utils)_ Add hash_file utility for SHA-256 file hashing - ([73a3aff](https://github.com/szinn/BookBoss/commit/73a3affe5d0ac07c016ee57c1e9964ee24561849))

### Bug Fixes

- _(formats)_ Migrate quick-xml unescape to decode for v0.39 - ([a19a8f3](https://github.com/szinn/BookBoss/commit/a19a8f3d82a085784e14b952e5e27a3fd63a6973))

### Refactor

- _(metadata,core)_ Provider chain with name() and create_metadata_providers() - ([d816f68](https://github.com/szinn/BookBoss/commit/d816f68b99c3d0c91b9de62a78715ad4c85bcfee))

### Testing

- _(formats)_ Add insta regression test suite for OPF parsing - ([5166ed2](https://github.com/szinn/BookBoss/commit/5166ed286a8391f83f8e6c67e7d5288d411551ca))

## [0.1.10](https://github.com/szinn/BookBoss/compare/v0.1.8..v0.1.10) - 2026-03-03

### Features

- _(core)_ Add ImportJobService with approve/reject transitions (M3.5) - ([fb894da](https://github.com/szinn/BookBoss/commit/fb894da2ce7ee25cfec5402abcad8b0a2811b388))
- _(core)_ Add pipeline port traits and models (M3.3) - ([8baa337](https://github.com/szinn/BookBoss/commit/8baa3379ebe48afe63c39060003785cc885575e5))
- _(core)_ Add LibraryStore port trait and BookSidecar struct (M3.2) - ([d6876ec](https://github.com/szinn/BookBoss/commit/d6876ec0d765880be614646afdf611851150e082))
- _(core,database)_ Add ImportJobRepository port and adapter (M3.4) - ([89afb54](https://github.com/szinn/BookBoss/commit/89afb5437508b6903994ef35db74c5d98507973d))
- _(database,core)_ Drop file_path from book_files (M3.1) - ([e785ee5](https://github.com/szinn/BookBoss/commit/e785ee57c22d94796c21eeab39d4d50aca33db67))
- _(frontend)_ Add series detail page (M2.7) - ([0bdfaec](https://github.com/szinn/BookBoss/commit/0bdfaec7da13d990eaebc2e3fb7805ebc5730657))
- _(frontend)_ Add author detail page (M2.6) - ([5cc4de8](https://github.com/szinn/BookBoss/commit/5cc4de8391f24c0cbc6d80632cd42e5d8239a99c))
- _(frontend)_ Add book detail page at /library/books/:token (M2.5) - ([9b1e670](https://github.com/szinn/BookBoss/commit/9b1e67083a091305514567cfad3d688d2f49428e))

### Bug Fixes

- _(frontend)_ Use #[post] instead of #[put] for get_book server fn - ([860943a](https://github.com/szinn/BookBoss/commit/860943a0a2eac9896106cb687b3912efe2d4b200))
- _(frontend)_ Require authentication on GET /api/v1/books - ([351c076](https://github.com/szinn/BookBoss/commit/351c0762f914abe4883355ecd335dd1bfe2128b3))

### Miscellaneous Tasks

- _(Dockerfile)_ Don't worry about target labels yet - ([613c637](https://github.com/szinn/BookBoss/commit/613c6372f0c5f9945ad58ba9c1be2143c28e0ac4))
- _(database)_ Squash drop migration - ([244acc1](https://github.com/szinn/BookBoss/commit/244acc13ccb83d3ee4ff4edd22dcc9bceba4749b))

## [0.1.8](https://github.com/szinn/BookBoss/compare/v0.1.7..v0.1.8) - 2026-03-03

### Features

- _(core)_ Implement BookService + wire into CoreServices (M2.3) - ([8e1e158](https://github.com/szinn/BookBoss/commit/8e1e158f010ba4235894016ddad5313e7d6d5cc0))
- _(core)_ Add import domain models and port trait (M1.8) - ([f85b71a](https://github.com/szinn/BookBoss/commit/f85b71a19a1e470005277339f6f9973e9862a6b5))
- _(core)_ Add shelf domain models and port trait (M1.7) - ([0544c9c](https://github.com/szinn/BookBoss/commit/0544c9cdf9fa31200ef2a4f0fb0b2a79e6e7fd9b))
- _(core,database)_ Implement BookRepositoryAdapter (M2.2) - ([fb0a169](https://github.com/szinn/BookBoss/commit/fb0a16950c84c8c491d69fadc47621649d34bc1a))
- _(core,database)_ Implement M2.1 reference table adapters - ([4fc67d5](https://github.com/szinn/BookBoss/commit/4fc67d56bb5be3708c83d20ec14f2dd15c41dc2c))
- _(core,database)_ Add user state and device table migrations and entities - ([f7f64ba](https://github.com/szinn/BookBoss/commit/f7f64baa55c3dbb1b4dfab26c16109244a9970dd))
- _(core,database)_ Add core book table migrations and entities - ([dc0e38a](https://github.com/szinn/BookBoss/commit/dc0e38a3f743035cb717650607630253fe063750))
- _(core,database)_ Add catalog reference table migrations and entities - ([e1a260e](https://github.com/szinn/BookBoss/commit/e1a260ea387f9b0a06576ddef69f869d314ec8f5))
- _(core,frontend)_ Add SuperAdmin capability and display app version - ([d4f6a3a](https://github.com/szinn/BookBoss/commit/d4f6a3ac018eafcdecc9074c6f9107d324948afb))
- _(frontend)_ Wire library page to real book data (M2.4) - ([e0bea77](https://github.com/szinn/BookBoss/commit/e0bea77889dde988a22ba70aa2a2a3c9447be3c8))

### Documentation

- Add project README - ([6a6d984](https://github.com/szinn/BookBoss/commit/6a6d9841d2f055ec934237b49b465ef666535745))

### Testing

- _(core)_ Add serde round-trip tests for ReadStatus and ShelfFilter - ([83fedec](https://github.com/szinn/BookBoss/commit/83fedecdb39fdd51f0afd631ffba2dc258335abb))

## [0.1.7] - 2026-03-01

### Features

- _(core)_ Add user domain with service, repository port, and tests - ([ddf56d4](https://github.com/szinn/BookBoss/commit/ddf56d46789a6e23e146787c7afdbc4a3b1e5f8c))
- _(core, database)_ Add auth domain with session management and login validation - ([56053ee](https://github.com/szinn/BookBoss/commit/56053eea4368e5c89f778067c06ddcfa6653d370))
- _(core, frontend)_ Add auth guard, Capability::as_str, and permission improvements - ([5cfc635](https://github.com/szinn/BookBoss/commit/5cfc6359d2eef2f30253dc8131b15f7a280a5d32))
- _(core,database)_ Add per-user key/value settings store - ([362e3ea](https://github.com/szinn/BookBoss/commit/362e3ead42e391a7c2dfbd30ce2227077fce703a))
- _(frontend)_ Add settings page with About section - ([bb4ba61](https://github.com/szinn/BookBoss/commit/bb4ba61a13d2cbac8f1773ba6a4777ed8dd713f1))
- _(frontend)_ Add GridView and NavBar view toggle for library - ([1e67cfe](https://github.com/szinn/BookBoss/commit/1e67cfef8db2c457c15aec7b21d4f04e97aee870))
- _(frontend)_ Add BookBoss title image to NavBar - ([9af471e](https://github.com/szinn/BookBoss/commit/9af471e20774bbda748d43314f03eb9927f1c40b))
- _(frontend)_ Add typed user settings and AuthUser get/set API - ([9e0a6d6](https://github.com/szinn/BookBoss/commit/9e0a6d637bfa8cbebd94733a0a1a8c7051388c8d))
- _(frontend)_ Replace NavBar text buttons with SVG icons - ([b73aacc](https://github.com/szinn/BookBoss/commit/b73aacc216a04dbf9bcf11301896f3683bb78f62))
- _(frontend)_ Add landing page with login and admin registration - ([f2d6f9f](https://github.com/szinn/BookBoss/commit/f2d6f9f7774129846be57db2b59a8b990d979465))
- _(frontend)_ Wire BackendSessionPool to AuthService and UserService - ([acfc6dc](https://github.com/szinn/BookBoss/commit/acfc6dc8107177307ab711cc594dab85b36d2572))
- _(frontend)_ Add favicon support - ([adfb6c7](https://github.com/szinn/BookBoss/commit/adfb6c70c3570e62b081dbd7693fe14ec99d2c4c))
- _(grpc)_ Move GRPC server to the grpc feature flag - ([07a1985](https://github.com/szinn/BookBoss/commit/07a1985a0ff6bb5a83d394c9d263a2dddc11d13f))
- _(metadata)_ Add stub crate for metadata services - ([d0ae6d6](https://github.com/szinn/BookBoss/commit/d0ae6d697c4409b40c161b6a253375435ab2ca22))

### Refactor

- _(frontend)_ Cleaning up and refactor - ([5f44656](https://github.com/szinn/BookBoss/commit/5f44656712f1a9587d2108a2c711ff0c0d1b66ca))
- _(frontend)_ Simplify extension extraction - ([8615bc4](https://github.com/szinn/BookBoss/commit/8615bc49334fbaab614b96af3563ce212c782000))
- _(frontend)_ Move functionality to real server module - ([2ad8409](https://github.com/szinn/BookBoss/commit/2ad8409b653e7d3d73da482df709a7d890c79b10))
- _(token)_ Randomize the alphabet for obscurity - ([6b0bf04](https://github.com/szinn/BookBoss/commit/6b0bf04f83e29dd2662784e516803aa108cb0acf))

### Documentation

- Add mdbook documentation with user guide and contributor sections - ([41de843](https://github.com/szinn/BookBoss/commit/41de8435fa6b56c1feb80dd888f18be8bcb3e5f2))

### Testing

- _(core, database)_ Add unit and component tests for User model and adapters - ([c0a1e48](https://github.com/szinn/BookBoss/commit/c0a1e4828978446aa64a840f16374610a6448548))

### Miscellaneous Tasks

- _(release)_ Add release script and GitHub Actions workflows - ([962f3ec](https://github.com/szinn/BookBoss/commit/962f3ec80bec55e86a83b0ff870d44aaf7d8a421))
