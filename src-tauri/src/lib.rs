//! Kolibri – Tauri-Backend.

mod accounts;
mod calendar;
mod compose;
mod contacts;
mod idle;
mod keychain;
mod messages;
mod notify;
mod queue;
mod startup;
mod sync;

use kolibri_store::Store;
use tauri::Manager;

pub use startup::mark_process_start;

/// Baut die Tauri-App und startet die Event-Schleife.
pub fn run() -> std::result::Result<(), tauri::Error> {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            startup::report_ready,
            startup::report_frontend_error,
            accounts::discover_settings,
            accounts::add_account,
            accounts::list_accounts,
            accounts::refresh_folders,
            accounts::remove_account,
            messages::list_messages,
            messages::get_message,
            messages::trust_image_sender,
            messages::sync_all,
            messages::sync_account,
            messages::get_thread,
            messages::unread_counts,
            messages::search_messages,
            contacts::list_contacts,
            contacts::suggest_addresses,
            contacts::save_contact,
            contacts::delete_contact,
            contacts::contact_accounts,
            contacts::list_collected,
            contacts::forget_collected,
            calendar::list_calendars,
            calendar::set_calendar_visible,
            calendar::create_calendar,
            calendar::update_calendar,
            calendar::delete_calendar,
            calendar::calendar_accounts,
            calendar::list_dav_sources,
            calendar::link_account_calendar,
            calendar::connect_dav_source,
            calendar::disconnect_dav_source,
            calendar::occurrences,
            calendar::event_detail,
            calendar::save_event,
            calendar::delete_event,
            calendar::respond_invite,
            queue::apply_action,
            queue::dismiss_failed,
            compose::prepare_compose,
            compose::send_message,
            compose::save_draft,
            compose::discard_draft,
            compose::render_preview,
            compose::list_signatures,
            compose::save_signature,
            compose::delete_signature,
            compose::set_default_signature,
            compose::list_templates,
            compose::save_template,
            compose::delete_template,
            compose::file_infos,
            compose::open_attachment,
            compose::save_attachment,
        ])
        .setup(|app| {
            startup::mark_phase("SETUP");
            // Protokoll: in der Entwicklung ausführlich auf die Konsole, im
            // Release nur Warnungen/Fehler nach ~/Library/Logs/app.kolibri.mail/.
            // Es werden nie Passwörter oder Mail-Inhalte protokolliert.
            let log = if cfg!(debug_assertions) {
                tauri_plugin_log::Builder::default().level(log::LevelFilter::Info)
            } else {
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Warn)
                    .clear_targets()
                    .target(tauri_plugin_log::Target::new(
                        tauri_plugin_log::TargetKind::LogDir { file_name: None },
                    ))
                    .max_file_size(1_000_000)
            };
            app.handle().plugin(log.build())?;

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = Store::open(&data_dir.join("kolibri.sqlite3"))?;
            app.manage(accounts::AppState::new(store));

            compose::clear_attachment_cache(app.handle());
            idle::start_all(app.handle());
            notify::update_badge(app.handle());
            startup::spawn_fallback_show(app.handle());
            Ok(())
        })
        // macOS wie Apple Mail: Fenster schließen blendet nur aus, Kolibri bleibt für
        // Push und Mitteilungen aktiv. Beenden mit ⌘Q, Klick aufs Dock zeigt es wieder.
        // Windows hat kein Dock zum Zurückholen – dort beendet Schließen die App.
        .on_window_event(|window, event| {
            if cfg!(target_os = "macos")
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())?
        .run(|_app, _event| {
            // Klick aufs Dock-Symbol zeigt das ausgeblendete Fenster wieder.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event
                && let Some(w) = _app.get_webview_window("main")
            {
                let _ = w.show();
                let _ = w.set_focus();
            }
        });
    Ok(())
}
