//! Integrationstests gegen einen echten (lokalen) Testserver – nie gegen ein
//! privates Postfach. Ohne Umgebungsvariablen werden sie übersprungen.
//!
//! Start: `scripts/test-server.sh` und dann
//! `KOLIBRI_TEST_SERVER=1 cargo test -p kolibri-mail --test live_server`

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use kolibri_core::{ErrorKind, FolderRole, MailProvider, Security, ServerConfig};
use kolibri_mail::{ImapSession, smtp};

struct Env {
    imap: ServerConfig,
    smtp: ServerConfig,
    password: String,
}

fn env() -> Option<Env> {
    std::env::var("KOLIBRI_TEST_SERVER").ok()?;
    let var = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_owned());
    let user = var("KOLIBRI_TEST_USER", "test@kolibri.test");
    let cfg = |port: &str| ServerConfig {
        host: var("KOLIBRI_TEST_HOST", "localhost"),
        port: port.parse().unwrap(),
        security: Security::None,
        username: user.clone(),
    };
    Some(Env {
        imap: cfg(&var("KOLIBRI_TEST_IMAP_PORT", "1143")),
        smtp: cfg(&var("KOLIBRI_TEST_SMTP_PORT", "1587")),
        password: var("KOLIBRI_TEST_PASSWORD", "kolibri-test"),
    })
}

macro_rules! require_server {
    () => {
        match env() {
            Some(e) => e,
            None => {
                eprintln!("übersprungen: KOLIBRI_TEST_SERVER nicht gesetzt");
                return;
            }
        }
    };
}

#[tokio::test]
async fn imap_login_and_list_folders() {
    let env = require_server!();
    let mut session = ImapSession::connect(&env.imap, &env.password)
        .await
        .unwrap();
    let folders = session.list_folders().await.unwrap();
    session.logout().await.unwrap();

    let inbox = folders
        .iter()
        .find(|f| f.role == Some(FolderRole::Inbox))
        .expect("INBOX fehlt");
    assert!(inbox.selectable);
    assert!(
        folders.len() >= 2,
        "erwartet mehr als nur INBOX: {folders:?}"
    );
}

#[tokio::test]
async fn imap_wrong_password_is_auth_error() {
    let env = require_server!();
    let err = ImapSession::connect(&env.imap, "falsch")
        .await
        .err()
        .unwrap();
    assert_eq!(err.kind, ErrorKind::Auth, "{err}");
}

#[tokio::test]
async fn smtp_login() {
    let env = require_server!();
    smtp::test_login(&env.smtp, &env.password).await.unwrap();
    let err = smtp::test_login(&env.smtp, "falsch").await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Auth, "{err}");
}

#[tokio::test]
async fn unreachable_port_is_connect_error() {
    let _env = require_server!();
    let cfg = ServerConfig {
        host: "localhost".into(),
        port: 1,
        security: Security::None,
        username: "x".into(),
    };
    let err = ImapSession::connect(&cfg, "x").await.err().unwrap();
    assert_eq!(err.kind, ErrorKind::Connect, "{err}");
}

#[tokio::test]
async fn idle_reports_new_mail_quickly() {
    let env = require_server!();
    let watcher = ImapSession::connect(&env.imap, &env.password)
        .await
        .unwrap();
    let imap = env.imap.clone();
    let password = env.password.clone();
    let sender = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let mut s = ImapSession::connect(&imap, &password).await.unwrap();
        let raw =
            b"From: a@kolibri.test\r\nTo: test@kolibri.test\r\nSubject: IDLE-Test\r\n\r\nHallo\r\n";
        s.append("INBOX", "()", raw).await.unwrap();
        s.logout().await.unwrap();
    });
    let started = std::time::Instant::now();
    let (watcher, changed) = watcher
        .idle_wait("INBOX", std::time::Duration::from_secs(20))
        .await
        .unwrap();
    sender.await.unwrap();
    assert!(changed, "IDLE hat die neue Mail nicht gemeldet");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );

    // Ohne Änderung: Zeitüberschreitung, Sitzung bleibt benutzbar.
    let (mut watcher, changed) = watcher
        .idle_wait("INBOX", std::time::Duration::from_secs(1))
        .await
        .unwrap();
    assert!(!changed);
    assert!(watcher.supports_idle().await.unwrap());
    watcher.logout().await.unwrap();
}
