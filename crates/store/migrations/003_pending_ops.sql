-- M4: Offline-Warteschlange.
-- Jede Aktion (gelesen, markieren, verschieben, löschen) wird sofort lokal
-- angewendet und hier eingereiht. Ein Hintergrund-Arbeiter überträgt sie an
-- den Server, sobald eine Verbindung besteht – in der Reihenfolge des Eintrags.

-- Nachrichten, die gerade verschoben/gelöscht werden, sind bis zur Bestätigung
-- durch den Server ausgeblendet (statt sofort gelöscht → rückgängig machbar).
ALTER TABLE messages ADD COLUMN pending_move INTEGER NOT NULL DEFAULT 0;

CREATE TABLE pending_ops (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id   INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uids        TEXT    NOT NULL,      -- JSON [uid, …]
    action      TEXT    NOT NULL,      -- JSON, siehe kolibri_core::Action
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    attempts    INTEGER NOT NULL DEFAULT 0,
    last_error  TEXT,
    failed      INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX pending_ops_account ON pending_ops(account_id, failed, id);
