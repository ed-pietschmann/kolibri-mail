//! Schema-Migrationen über SQLites `PRAGMA user_version`.
//!
//! Regeln: Migrationen werden nur angehängt, nie geändert. Jede läuft in einer
//! eigenen Transaktion zusammen mit dem Hochsetzen der Version.

use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_accounts_folders.sql"),
    include_str!("../migrations/002_messages.sql"),
    include_str!("../migrations/003_pending_ops.sql"),
    include_str!("../migrations/004_compose.sql"),
    include_str!("../migrations/005_threads.sql"),
    include_str!("../migrations/006_search.sql"),
    include_str!("../migrations/007_contacts.sql"),
    include_str!("../migrations/008_calendar.sql"),
    include_str!("../migrations/009_signatures_templates.sql"),
    include_str!("../migrations/010_contacts_calendars.sql"),
    include_str!("../migrations/011_designs.sql"),
    include_str!("../migrations/012_dav_retry.sql"),
    include_str!("../migrations/013_designs_i18n.sql"),
];

pub fn run(conn: &mut Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (index, sql) in MIGRATIONS
        .iter()
        .enumerate()
        .skip(usize::try_from(current).unwrap_or(0))
    {
        let version = i64::try_from(index + 1).unwrap_or(i64::MAX);
        log::info!("Datenbank-Migration {version}");
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        run(&mut conn).unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(usize::try_from(v).unwrap(), MIGRATIONS.len());
    }
}
