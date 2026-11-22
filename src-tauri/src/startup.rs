//! Startzeit-Messung und verzögertes Einblenden des Fensters.
//!
//! Das Fenster startet unsichtbar (`visible: false`) und wird erst gezeigt,
//! wenn das Frontend seinen ersten Frame gezeichnet hat. So gibt es kein
//! weißes Aufblitzen. Ein Sicherheits-Timer zeigt es spätestens nach 3 s.

use std::io::Write;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, WebviewWindow};

use kolibri_core::{Error, ErrorKind, Result};

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

pub const MAIN_WINDOW: &str = "main";
const FALLBACK_SHOW_AFTER: Duration = Duration::from_secs(3);

/// So früh wie möglich in `main()` aufrufen.
pub fn mark_process_start() {
    let _ = PROCESS_START.set(Instant::now());
}

fn elapsed_ms() -> u64 {
    PROCESS_START
        .get()
        .map(|start| u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Schreibt eine maschinenlesbare Zeitmarke (für scripts/measure.sh).
pub fn mark_phase(name: &str) {
    let ms = elapsed_ms();
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "KOLIBRI_{name}_MS={ms}");
    let _ = out.flush();
}

/// Vom Frontend nach dem ersten gezeichneten Frame aufgerufen.
#[tauri::command]
pub fn report_ready(window: WebviewWindow, page_ms: f64) -> Result<u64> {
    let win_err = |e: tauri::Error| Error::new(ErrorKind::Internal, e.to_string());
    let ms = elapsed_ms();
    log::info!("Seite brauchte {page_ms:.0} ms seit Navigationsbeginn");
    if !window.is_visible().map_err(win_err)? {
        window.show().map_err(win_err)?;
        window.set_focus().map_err(win_err)?;
        // Maschinenlesbare Zeile für scripts/measure.sh
        mark_phase("READY");
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "KOLIBRI_PAGE_MS={page_ms:.0}");
        let _ = out.flush();
        log::info!("Fenster sichtbar nach {ms} ms");
    }
    #[cfg(debug_assertions)]
    run_debug_script(&window);
    Ok(ms)
}

/// Fehler aus der Oberfläche (JavaScript) ins Protokoll schreiben, damit sie im
/// Release-Build (ohne Entwicklerwerkzeuge) nachvollziehbar sind.
#[tauri::command]
pub fn report_frontend_error(message: String) {
    let short: String = message.chars().take(2000).collect();
    log::error!("Oberfläche: {short}");
}

/// Nur Debug-Build: `KOLIBRI_EVAL=<datei.js>` führt nach dem Start ein Skript in
/// der Oberfläche aus (automatisierte UI-Prüfung ohne Maus).
#[cfg(debug_assertions)]
fn run_debug_script(window: &WebviewWindow) {
    let Ok(path) = std::env::var("KOLIBRI_EVAL") else {
        return;
    };
    match std::fs::read_to_string(&path) {
        Ok(js) => {
            if let Err(e) = window.eval(&js) {
                log::error!("KOLIBRI_EVAL: {e}");
            }
        }
        Err(e) => log::error!("KOLIBRI_EVAL {path}: {e}"),
    }
}

/// Zeigt das Fenster auch dann, wenn das Frontend sich nie meldet.
pub fn spawn_fallback_show(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FALLBACK_SHOW_AFTER);
        if let Some(window) = app.get_webview_window(MAIN_WINDOW)
            && !window.is_visible().unwrap_or(true)
        {
            log::warn!("Frontend hat sich nicht gemeldet – Fenster wird trotzdem gezeigt");
            let _ = window.show();
        }
    });
}
