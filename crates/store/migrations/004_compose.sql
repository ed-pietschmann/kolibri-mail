-- M5: Signatur und Postausgang.

ALTER TABLE accounts ADD COLUMN signature TEXT NOT NULL DEFAULT '';

-- Ausgehende Nachrichten und Entwürfe. Die fertige MIME-Nachricht wird beim
-- Absenden gebaut und hier gespeichert; ein Arbeiter überträgt sie, sobald
-- eine Verbindung besteht (auch nach Neustart).
--   kind = 'send'    → per SMTP senden, danach wird daraus 'sent'
--   kind = 'sent'    → gesendet; noch Kopie in „Gesendet“ ablegen + Entwurf löschen
--   kind = 'draft'   → alten Entwurf (gleiche Message-ID) ersetzen
--   kind = 'discard' → Entwurf mit dieser Message-ID löschen
CREATE TABLE outbox (
    id              INTEGER PRIMARY KEY,
    account_id      INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind            TEXT    NOT NULL,
    message_id      TEXT    NOT NULL,     -- ohne <>; stabil über alle Entwurfsfassungen
    raw             BLOB,
    envelope_from   TEXT    NOT NULL DEFAULT '',
    envelope_to     TEXT    NOT NULL DEFAULT '[]',   -- JSON [addr, …]
    answered_id     INTEGER,              -- lokale Nachricht, auf die geantwortet wurde
    subject         TEXT    NOT NULL DEFAULT '',
    created_at      INTEGER NOT NULL DEFAULT (unixepoch()),
    attempts        INTEGER NOT NULL DEFAULT 0,
    last_error      TEXT,
    failed          INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX outbox_account ON outbox(account_id, failed, id);

-- Antwortadresse (Reply-To), falls abweichend vom Absender.
ALTER TABLE messages ADD COLUMN reply_to TEXT NOT NULL DEFAULT '[]';
