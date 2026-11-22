#!/usr/bin/env bash
# Kolibri aktualisieren: neue Version bauen, laufende App beenden,
# in /Applications ersetzen und neu starten. Daten und Passwörter bleiben erhalten
# (sie liegen in ~/Library bzw. im Schlüsselbund, nicht in der App).
#
#   pnpm update-app            bauen + installieren
#   pnpm update-app --no-build zuletzt gebaute Version installieren
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_SRC="$ROOT/target/release/bundle/macos/Kolibri.app"
# KOLIBRI_APP_DST nur zum Testen (anderer Zielordner, App wird dann nicht beendet/gestartet).
APP_DST="${KOLIBRI_APP_DST:-/Applications/Kolibri.app}"
TEST_RUN=$([ -n "${KOLIBRI_APP_DST:-}" ] && echo 1 || echo 0)

cd "$ROOT"
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

if [ "${1:-}" != "--no-build" ]; then
  echo "▸ Baue neue Version …"
  CI=true pnpm tauri build --bundles app >/dev/null
fi
[ -d "$APP_SRC" ] || { echo "Kein Build gefunden: $APP_SRC" >&2; exit 1; }
VERSION=$(defaults read "$APP_SRC/Contents/Info.plist" CFBundleShortVersionString)

if [ "$TEST_RUN" = 0 ] && pgrep -xq kolibri; then
  echo "▸ Beende Kolibri …"
  osascript -e 'tell application id "app.kolibri.mail" to quit' >/dev/null 2>&1 || true
  for _ in $(seq 1 20); do pgrep -xq kolibri || break; sleep 0.25; done
  pgrep -xq kolibri && pkill -x kolibri || true
fi

echo "▸ Installiere Version $VERSION nach $APP_DST …"
rm -rf "$APP_DST"
ditto "$APP_SRC" "$APP_DST"

if [ "$TEST_RUN" = 0 ]; then
  echo "▸ Starte Kolibri …"
  open "$APP_DST"
fi
echo "✓ Kolibri $VERSION ist installiert."
echo "  Hinweis: macOS fragt nach einem Update evtl. einmal nach dem Schlüsselbund – „Immer erlauben“."
