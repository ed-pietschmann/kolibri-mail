//! Mitteilungen bei neuen Mails und Zahl im Dock-Symbol.

use kolibri_core::{FolderRole, MessageSummary};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::accounts::AppState;

/// Mehr als so viele neue Mails → eine Sammelmitteilung.
const MAX_SINGLE: usize = 3;

fn window_focused(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

/// Mitteilung für neue, ungelesene Mails – nur wenn Kolibri nicht vorne ist.
pub fn new_mail(app: &AppHandle, fresh: &[MessageSummary]) {
    if fresh.is_empty() || window_focused(app) {
        return;
    }
    let show = |title: &str, body: &str| {
        if let Err(e) = app.notification().builder().title(title).body(body).show() {
            log::warn!("Mitteilung nicht möglich: {e}");
        }
    };
    if fresh.len() > MAX_SINGLE {
        show("Kolibri", &format!("{} neue Nachrichten", fresh.len()));
        return;
    }
    for m in fresh {
        let sender = if m.from_name.trim().is_empty() {
            &m.from_addr
        } else {
            &m.from_name
        };
        let subject = if m.subject.trim().is_empty() {
            "(Kein Betreff)"
        } else {
            &m.subject
        };
        show(sender, subject);
    }
}

/// Dock-Symbol: Zahl ungelesener Mails in allen Posteingängen.
pub fn update_badge(app: &AppHandle) {
    let state = app.state::<AppState>();
    let inboxes: Vec<_> = state
        .store
        .accounts()
        .unwrap_or_default()
        .iter()
        .filter_map(|a| {
            state
                .store
                .folder_by_role(a.id, FolderRole::Inbox)
                .ok()
                .flatten()
        })
        .map(|f| f.id)
        .collect();
    let unread: u32 = state
        .store
        .unread_counts()
        .unwrap_or_default()
        .into_iter()
        .filter(|(f, _)| inboxes.contains(f))
        .map(|(_, n)| n)
        .sum();
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_badge_count((unread > 0).then_some(i64::from(unread)));
    }
}
