// Verhindert ein zusätzliches Konsolenfenster unter Windows im Release-Build.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    kolibri_lib::mark_process_start();
    if let Err(err) = kolibri_lib::run() {
        eprintln!("Kolibri konnte nicht gestartet werden: {err}");
        std::process::exit(1);
    }
}
