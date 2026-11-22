import { mount, tick } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import './app.css';
import App from './App.svelte';
import { isMac } from './lib/platform';

// Plattform für CSS (z. B. kein Platz für die Mac-Fensterknöpfe unter Windows).
document.documentElement.dataset['platform'] = isMac ? 'mac' : 'windows';

// Browser-Vorschau ohne Tauri: Backend-Attrappe laden (nur Entwicklung).
if (import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)) {
  await import('./dev/mock');
}

// Fehler der Oberfläche ins Protokoll der App (~/Library/Logs/app.kolibri.mail/).
function reportError(what: unknown) {
  const text =
    what instanceof Error ? `${what.message}\n${what.stack ?? ''}` : JSON.stringify(what);
  invoke('report_frontend_error', { message: text }).catch(() => {});
}
window.addEventListener('error', (e) => reportError(e.error ?? e.message));
window.addEventListener('unhandledrejection', (e) => reportError(e.reason));

const target = document.getElementById('app');
if (!target) throw new Error('#app fehlt in index.html');

mount(App, { target });

// Sobald das DOM steht, darf das Backend das (bis dahin unsichtbare) Fenster
// zeigen. Achtung: requestAnimationFrame feuert in unsichtbaren Fenstern nicht
// – deshalb hier tick() statt rAF.
void tick().then(() =>
  invoke<number>('report_ready', { pageMs: performance.now() }).catch(() => {
    // Läuft im normalen Browser (pnpm dev) – kein Tauri vorhanden.
  }),
);
