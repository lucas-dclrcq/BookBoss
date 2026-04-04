# KOReader Sync

BookBoss implements the KOReader sync server protocol so KOReader devices can sync reading
position with your library. Books are delivered via [OPDS](opds.md) — KOReader sync handles
reading progress only. No books are pushed to the device; KOReader downloads them from the
OPDS catalog.

## Setup

### 1. Enable OPDS Access

KOReader sync uses the same credentials as OPDS. If you have not already done so, enable
OPDS access on your **Profile** page. This generates the password KOReader will use.

### 2. Find Your Sync URL and Password

On your **Profile** page, under the **OPDS / KOReader** section:

- **Password** — the password KOReader will use to authenticate
- **KOReader Sync URL** — the URL to enter in KOReader's sync settings (e.g. `http://bookboss.local:8080/koreader`)

### 3. Configure KOReader

In KOReader, go to **Tools → KOReader Sync** (or the Progress Sync plugin) and enter:

| Field    | Value                                   |
| -------- | --------------------------------------- |
| Server   | Your KOReader Sync URL from the profile |
| Username | Your BookBoss username                  |
| Password | The password shown on your profile page |

Tap **Login / Sync** to verify the connection. KOReader will report a successful login if
the credentials are correct.

### 4. Add Books via OPDS

To get books onto your KOReader device, add BookBoss as an OPDS catalog source in KOReader
(see [OPDS Catalog](opds.md)) and download books from there. BookBoss registers the
document hashes at download time so KOReader sync can identify books by their file content.

## How Sync Works

When you read a book on KOReader:

1. KOReader sends your current reading position to BookBoss after each page turn
2. BookBoss updates your reading progress and reading state for that book
3. The next time you open the same book on any KOReader device connected to BookBoss, your
   position is restored

Sync is bidirectional within KOReader — BookBoss stores the last-pushed position per user
per book and returns it on pull.

### Reading State Transitions

BookBoss automatically updates your reading state based on progress pushed from KOReader:

| Condition                            | Effect                                           |
| ------------------------------------ | ------------------------------------------------ |
| Progress > 0% and status is _Unread_ | Status changes to **Reading**                    |
| Progress reaches 100%                | Status changes to **Read**; finish date recorded |
| All other cases                      | Status unchanged                                 |

Progress of 0% does not change your reading state, since this can occur when a book is
first opened or re-read.

## Supported Devices

Any KOReader version that supports the built-in **Progress Sync** plugin should work.
KOReader identifies books using an MD5 digest of either the filename or the file contents,
depending on the device configuration. BookBoss registers both digests at download time so
both modes are supported.

## Troubleshooting

**Login fails in KOReader**

- Confirm the sync URL ends with `/koreader` (no trailing slash)
- Confirm the username and password match exactly what is shown on the profile page
- Ensure your BookBoss instance is reachable from the device's network

**Position does not sync**

- The book must have been downloaded through BookBoss (via OPDS or the web UI) so that
  its hash is registered. Books sideloaded from other sources cannot be matched.
- Check that KOReader's Progress Sync plugin is enabled and configured

**Progress shows but reading state did not change**

- Reading state transitions only fire on a push (position sent from KOReader to BookBoss).
  Pull-only sessions do not trigger transitions.
