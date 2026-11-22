-- M2: Konten und Ordner.
-- Passwörter liegen NICHT hier, sondern im macOS-Schlüsselbund.

CREATE TABLE accounts (
    id             INTEGER PRIMARY KEY,
    email          TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    display_name   TEXT    NOT NULL,
    imap_host      TEXT    NOT NULL,
    imap_port      INTEGER NOT NULL,
    imap_security  TEXT    NOT NULL,
    imap_username  TEXT    NOT NULL,
    smtp_host      TEXT    NOT NULL,
    smtp_port      INTEGER NOT NULL,
    smtp_security  TEXT    NOT NULL,
    smtp_username  TEXT    NOT NULL,
    sort_order     INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE folders (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    path        TEXT    NOT NULL,
    name        TEXT    NOT NULL,
    delimiter   TEXT,
    role        TEXT,
    selectable  INTEGER NOT NULL,
    UNIQUE (account_id, path)
) STRICT;

CREATE INDEX folders_account ON folders(account_id);
