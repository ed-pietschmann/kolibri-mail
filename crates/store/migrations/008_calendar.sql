-- M9: Kalender und Termine.

ALTER TABLE accounts ADD COLUMN caldav_home TEXT;
ALTER TABLE accounts ADD COLUMN caldav_checked INTEGER;

-- Kalender: vom Server (account_id + url) oder lokal (account_id NULL).
CREATE TABLE calendars (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
    url         TEXT    NOT NULL,
    name        TEXT    NOT NULL,
    color       TEXT    NOT NULL DEFAULT '',
    visible     INTEGER NOT NULL DEFAULT 1,
    UNIQUE (account_id, url)
) STRICT;

-- Termine: ganze iCalendar-Datei (inkl. Ausnahmen einer Serie).
-- start_unix/end_bound grenzen die Bereichsabfrage ein; end_bound NULL = Serie ohne Ende.
CREATE TABLE events (
    id           INTEGER PRIMARY KEY,
    calendar_id  INTEGER NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    href         TEXT    NOT NULL,
    etag         TEXT    NOT NULL DEFAULT '',
    uid          TEXT    NOT NULL DEFAULT '',
    ics          TEXT    NOT NULL,
    start_unix   INTEGER NOT NULL,
    end_bound    INTEGER,
    UNIQUE (calendar_id, href)
) STRICT;

CREATE INDEX events_range ON events(start_unix, end_bound);

-- Einladung in einer Mail (JSON), erkannt beim Lesen des Inhalts.
ALTER TABLE message_bodies ADD COLUMN invite TEXT;

-- Der lokale Kalender „Auf diesem Mac“ existiert immer.
INSERT INTO calendars (account_id, url, name, color) VALUES (NULL, 'local', 'Auf diesem Mac', '#0a84ff');
