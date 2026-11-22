-- 1.3: eigene Kontakte, mehrere eigene Kalender, verbundene Kalender-Konten.

-- Kontakte dürfen lokal sein (account_id NULL). SQLite kann NOT NULL nicht
-- entfernen, also Tabelle neu anlegen und Daten übernehmen.
CREATE TABLE contacts_new (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    book_url    TEXT    NOT NULL,
    href        TEXT    NOT NULL,
    etag        TEXT    NOT NULL DEFAULT '',
    uid         TEXT    NOT NULL DEFAULT '',
    name        TEXT    NOT NULL DEFAULT '',
    org         TEXT    NOT NULL DEFAULT '',
    emails      TEXT    NOT NULL DEFAULT '[]',
    phones      TEXT    NOT NULL DEFAULT '[]',
    UNIQUE (account_id, href)
) STRICT;
INSERT INTO contacts_new SELECT id, account_id, book_url, href, etag, uid, name, org, emails, phones
FROM contacts;
DROP TABLE contacts;
ALTER TABLE contacts_new RENAME TO contacts;
CREATE INDEX contacts_name ON contacts(name COLLATE NOCASE);

-- Kalender aus anderen Konten (iCloud, Nextcloud …) per CalDAV-Adresse.
-- Passwort im Schlüsselbund unter „dav-<id>“.
CREATE TABLE dav_sources (
    id             INTEGER PRIMARY KEY,
    name           TEXT    NOT NULL,
    url            TEXT    NOT NULL,
    username       TEXT    NOT NULL,
    calendar_home  TEXT    NOT NULL DEFAULT '',
    created_at     INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

ALTER TABLE calendars ADD COLUMN source_id INTEGER REFERENCES dav_sources(id) ON DELETE CASCADE;

-- Erkennung von Kontakten/Kalendern wurde verbessert: Konten ohne Fund neu suchen.
UPDATE accounts SET carddav_home = NULL, carddav_checked = NULL WHERE carddav_home = '';
UPDATE accounts SET caldav_home = NULL, caldav_checked = NULL WHERE caldav_home = '';
