-- M6: Konversationen (Threads).
-- thread_root = Message-ID der ersten Nachricht des Gesprächs (ohne <>).
-- Wird beim Einfügen aus References/In-Reply-To bestimmt; bestehende Zeilen
-- füllt Store::backfill_threads() nach (in Rust, weil die Kopfzeilen geparst werden).
ALTER TABLE messages ADD COLUMN thread_root TEXT;
CREATE INDEX messages_thread ON messages(account_id, thread_root);
CREATE INDEX messages_msgid ON messages(account_id, message_id);
