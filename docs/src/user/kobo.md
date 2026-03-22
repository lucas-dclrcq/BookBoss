# Kobo Device Sync

BookBoss can sync books and reading progress directly to Kobo e-readers. Your Kobo connects to
BookBoss as if it were the Kobo store, receiving books and syncing reading state.

## Registering a Device

1. Go to your **Profile** page
2. Click **Add Device** and give it a name
3. Copy the **sync URL** — you will need to configure your Kobo to use this URL

### Configuring Your Kobo

The sync URL must be set in your Kobo's configuration database. The exact setup depends on your
Kobo model and firmware version. The sync URL replaces the default Kobo store API endpoint so that
the device communicates with BookBoss instead.

## How Sync Works

Each registered Kobo device is paired with a **companion smart shelf**. To sync books to a device:

1. Edit the companion shelf's filter rules to select the books you want to sync (e.g. by reading status, author, genre, or tag)
2. The next time your Kobo syncs, it will download the matching books

Sync is **incremental** — only new or changed books are sent each time the device connects.

### Supported Formats

- **EPUB** — served as-is
- **KEPUB** — BookBoss converts EPUBs to KEPUB format automatically for optimal Kobo rendering

### Reading State Sync

Reading progress syncs bidirectionally between your Kobo and BookBoss:

- **Position** — your current reading position in the book
- **Progress** — percentage complete
- **Time spent** — reading time tracked by the Kobo

When you read on your Kobo, the progress appears in BookBoss. Reading state changes in BookBoss
are reflected on the device at next sync.

## Managing Devices

From your **Profile** page you can:

- **Copy sync URL** — copy the device sync URL to your clipboard
- **Reset sync** — force a full re-sync (the device will re-download all books)
- **Configure on-removal action** — choose what happens when a book is removed from the device

## Troubleshooting

If your Kobo is not syncing:

- Ensure the Kobo is connected to the same network as your BookBoss instance
- Verify the sync URL is correctly configured on the device
- Check that books have been added to the device's companion shelf
- Try resetting the sync state from the Profile page
