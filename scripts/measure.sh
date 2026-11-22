#!/usr/bin/env bash
# Misst Kolibri im Release-Build: Startzeit, RAM und CPU im Leerlauf, Bundle-Größe.
#
# Nutzung:  pnpm measure            (baut vorher)
#           pnpm measure --no-build (nimmt den vorhandenen Build)
#
# RAM = App-Prozess + die WebKit-Hilfsprozesse (WebContent, Networking, GPU),
# die beim Start neu entstehen. Activity Monitor zeigt diese getrennt an.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/target/release/bundle/macos/Kolibri.app"
BIN="$APP/Contents/MacOS/kolibri"
IDLE_WAIT=${IDLE_WAIT:-8}

if [[ "${1:-}" != "--no-build" ]]; then
  (cd "$ROOT" && pnpm tauri build --bundles app >/dev/null 2>&1)
fi
[[ -x "$BIN" ]] || { echo "Build fehlt: $BIN" >&2; exit 1; }

webkit_pids() { pgrep -f 'com.apple.WebKit.(WebContent|Networking|GPU)' | sort || true; }

log=$(mktemp)
APP_PID=""
trap 'kill $APP_PID 2>/dev/null || true; rm -f "$log"' EXIT

# Startet die App und wartet auf das "Fenster sichtbar"-Signal (max. 10 s).
# Setzt APP_PID und READY (keine Unter-Shell, sonst geht die PID verloren).
READY=""
launch() {
  "$BIN" >"$log" 2>&1 &
  APP_PID=$!
  for _ in $(seq 1 100); do
    grep -q KOLIBRI_READY_MS "$log" && break
    sleep 0.1
  done
  READY=$(grep -o 'KOLIBRI_READY_MS=[0-9]*' "$log" | cut -d= -f2 || true)
}
stop() { kill "$APP_PID" 2>/dev/null || true; wait "$APP_PID" 2>/dev/null || true; sleep 0.5; }

# Aufwärm-Start: Nach jedem Neubau prüft macOS das neue Binary einmalig
# (Signatur, XProtect). Das zählt nicht zur echten Startzeit.
launch
stop

# Startzeit = Median aus drei Starts
times=()
for _ in 1 2 3; do
  launch
  times+=("$READY")
  stop
done
ready_ms=$(printf '%s\n' "${times[@]}" | sort -n | sed -n 2p)

# Letzter Start bleibt für die Leerlauf-Messung offen
before=$(webkit_pids)
launch
# Kolibris WebKit-Hilfsprozesse entstehen beim Öffnen des Fensters. Nur die
# zählen, die bis kurz danach neu sind – sonst würden WebKit-Prozesse anderer
# Apps (Safari, Mail …), die während der Wartezeit starten, mitgezählt.
sleep 2
after=$(webkit_pids)
helpers=$(comm -13 <(echo "$before") <(echo "$after") | tr '\n' ' ')

sleep "$IDLE_WAIT"
# Nur noch laufende Hilfsprozesse auswerten.
alive=""
for p in $helpers; do kill -0 "$p" 2>/dev/null && alive="$alive $p"; done
helpers=$alive

pid_args=(-pid "$APP_PID")
for p in $helpers; do pid_args+=(-pid "$p"); done

# Drei Messpunkte im Abstand von 3 s; ausgewertet wird der letzte.
sample=$(top -l 3 -s 3 -stats pid,command,cpu,mem "${pid_args[@]}" | awk '/^PID/{block=""} {block=block $0 "\n"} END{printf "%s", block}')

to_mb() {
  local v=$1
  case "$v" in
    *G*) echo "${v%%G*} * 1024" | bc ;;
    *M*) echo "${v%%M*}" ;;
    *K*) echo "scale=1; ${v%%K*} / 1024" | bc ;;
    *B*) echo "0" ;;
    *) echo "$v" ;;
  esac
}

total_mem=0
total_cpu=0
echo
echo "Prozess                          CPU %   RAM"
while read -r pid cmd cpu mem; do
  [[ "$pid" =~ ^[0-9]+$ ]] || continue
  mb=$(to_mb "${mem%[+-]}")
  total_mem=$(echo "$total_mem + $mb" | bc)
  total_cpu=$(echo "$total_cpu + $cpu" | bc)
  printf "  %-30s %5s   %s MB\n" "$cmd ($pid)" "$cpu" "$mb"
done <<<"$sample"

app_size=$(du -sh "$APP" | cut -f1)
dmg=$(ls "$ROOT"/target/release/bundle/dmg/*.dmg 2>/dev/null | head -1 || true)
dmg_size=$([[ -n "$dmg" ]] && du -sh "$dmg" | cut -f1 || echo "–")

echo
echo "Ergebnis"
echo "  Fenster sichtbar nach:  ${ready_ms:-?} ms (Median; Einzelwerte: ${times[*]})"
echo "  RAM gesamt (Leerlauf):  ${total_mem} MB   (Ziel < 150 MB)"
echo "  CPU gesamt (Leerlauf):  ${total_cpu} %    (Ziel < 1 %)"
echo "  App-Bundle:             ${app_size}"
echo "  DMG:                    ${dmg_size}"
