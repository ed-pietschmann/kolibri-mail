-- M3: Nachrichten-Cache.
-- Kopfdaten liegen in `messages` (klein, für die Liste), Inhalte getrennt in
-- `message_bodies` (groß, nur bei Bedarf gelesen). So bleibt die Liste schnell
-- und der Arbeitsspeicher klein.

ALTER TABLE folders ADD COLUMN uid_validity INTEGER;
ALTER TABLE folders ADD COLUMN uid_next INTEGER;
ALTER TABLE folders ADD COLUMN last_sync INTEGER;

CREATE TABLE messages (
    id              INTEGER PRIMARY KEY,
    account_id      INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id       INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid             INTEGER NOT NULL,
    message_id      TEXT,
    in_reply_to     TEXT,
    refs            TEXT,
    subject         TEXT    NOT NULL DEFAULT '',
    from_name       TEXT    NOT NULL DEFAULT '',
    from_addr       TEXT    NOT NULL DEFAULT '',
    to_addrs        TEXT    NOT NULL DEFAULT '[]',   -- JSON [{name, addr}]
    cc_addrs        TEXT    NOT NULL DEFAULT '[]',
    date            INTEGER NOT NULL,                -- Eingang (INTERNALDATE), Unix-Sekunden
    size            INTEGER NOT NULL DEFAULT 0,
    seen            INTEGER NOT NULL DEFAULT 0,
    flagged         INTEGER NOT NULL DEFAULT 0,
    answered        INTEGER NOT NULL DEFAULT 0,
    draft           INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    preview         TEXT    NOT NULL DEFAULT '',
    UNIQUE (folder_id, uid)
) STRICT;

CREATE INDEX messages_folder_date ON messages(folder_id, date DESC);
CREATE INDEX messages_flagged ON messages(flagged, date DESC) WHERE flagged = 1;

CREATE TABLE message_bodies (
    message_id     INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    html           TEXT,              -- bereits gesäubert (ammonia)
    text           TEXT,
    remote_images  INTEGER NOT NULL DEFAULT 0,   -- enthält externe Bilder?
    attachments    TEXT    NOT NULL DEFAULT '[]', -- JSON [{name, mime, size}]
    fetched_at     INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

-- Absender, deren externe Bilder automatisch geladen werden dürfen.
CREATE TABLE image_whitelist (
    address  TEXT PRIMARY KEY COLLATE NOCASE
) STRICT;
