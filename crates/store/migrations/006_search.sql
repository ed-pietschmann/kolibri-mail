-- M7: Volltextsuche (FTS5).
-- Inhaltslos (content=''): der Index speichert keine zweite Kopie der Texte,
-- nur die Suchstruktur – das hält die Datenbank klein. rowid = messages.id.
-- remove_diacritics 2: „Muller“ findet „Müller“. prefix: schnelle Präfixsuche.
CREATE VIRTUAL TABLE messages_fts USING fts5(
    subject, sender, recipients, body,
    content = '',
    contentless_delete = 1,
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3'
);

-- Neue Nachricht: Kopfdaten indizieren (Inhalt folgt, sobald geladen).
CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts (rowid, subject, sender, recipients, body)
    VALUES (new.id, new.subject, new.from_name || ' ' || new.from_addr,
            new.to_addrs || ' ' || new.cc_addrs, '');
END;

CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE rowid = old.id;
END;

-- Inhalt geladen: Zeile mit Text neu schreiben (höchstens 20 000 Zeichen –
-- reicht für jede normale Mail und begrenzt die Indexgröße bei Newslettern).
CREATE TRIGGER message_bodies_fts AFTER INSERT ON message_bodies BEGIN
    DELETE FROM messages_fts WHERE rowid = new.message_id;
    INSERT INTO messages_fts (rowid, subject, sender, recipients, body)
    SELECT m.id, m.subject, m.from_name || ' ' || m.from_addr,
           m.to_addrs || ' ' || m.cc_addrs, substr(COALESCE(new.text, ''), 1, 20000)
    FROM messages m WHERE m.id = new.message_id;
END;

-- Bestehende Nachrichten nachtragen.
INSERT INTO messages_fts (rowid, subject, sender, recipients, body)
SELECT m.id, m.subject, m.from_name || ' ' || m.from_addr,
       m.to_addrs || ' ' || m.cc_addrs, substr(COALESCE(b.text, ''), 1, 20000)
FROM messages m LEFT JOIN message_bodies b ON b.message_id = m.id;
