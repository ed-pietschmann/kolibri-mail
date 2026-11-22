-- M8: Kontakte (CardDAV) und gesammelte Adressen.

-- Adressbuch-Verzeichnis des Kontos: NULL = noch nicht gesucht, '' = keins gefunden.
ALTER TABLE accounts ADD COLUMN carddav_home TEXT;
ALTER TABLE accounts ADD COLUMN carddav_checked INTEGER;

CREATE TABLE contacts (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    book_url    TEXT    NOT NULL,
    href        TEXT    NOT NULL,      -- Pfad auf dem Server
    etag        TEXT    NOT NULL DEFAULT '',
    uid         TEXT    NOT NULL DEFAULT '',
    name        TEXT    NOT NULL DEFAULT '',
    org         TEXT    NOT NULL DEFAULT '',
    emails      TEXT    NOT NULL DEFAULT '[]',   -- JSON [{value, label}]
    phones      TEXT    NOT NULL DEFAULT '[]',
    UNIQUE (account_id, href)
) STRICT;

CREATE INDEX contacts_name ON contacts(name COLLATE NOCASE);

-- Adressen, an die man geschrieben hat („Vorherige Empfänger“ wie bei Apple Mail).
CREATE TABLE collected_addresses (
    address    TEXT    PRIMARY KEY COLLATE NOCASE,
    name       TEXT    NOT NULL DEFAULT '',
    use_count  INTEGER NOT NULL DEFAULT 0,
    last_used  INTEGER NOT NULL DEFAULT 0
) STRICT;
