#!/usr/bin/env bash
# Lokaler Mail-Testserver (Stalwart) – NUR für Tests, lauscht nur auf 127.0.0.1.
#
#   scripts/test-server.sh install   Stalwart herunterladen (einmalig)
#   scripts/test-server.sh reset     Daten löschen und frisch einrichten
#   scripts/test-server.sh start     starten
#   scripts/test-server.sh stop      stoppen
#   scripts/test-server.sh status    läuft er?
#
# Testkonto:  test@kolibri.test / kolibri-test
#   IMAP  127.0.0.1:1143  (ohne TLS)
#   SMTP  127.0.0.1:1587  (ohne TLS)
#   JMAP  127.0.0.1:18080 (Verwaltung / Testdaten einspielen)
#
# Tests:  KOLIBRI_TEST_SERVER=1 cargo test -p kolibri-mail --test live_server
set -euo pipefail

VERSION="v1.1.1"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="$ROOT/.dev/stalwart"
BIN="$DIR/bin/stalwart"
CONFIG="$DIR/config.json"
LOG="$DIR/server.log"
RECOVERY_PORT=18081

# Erkennung über den Konfigurationspfad – trifft jede Startart (absolut/relativ).
running() { pgrep -f "stalwart --config=$CONFIG" >/dev/null; }

wait_port() {
  for _ in $(seq 1 50); do
    nc -z 127.0.0.1 "$1" 2>/dev/null && return 0
    sleep 0.2
  done
  echo "Port $1 kommt nicht hoch – siehe $LOG" >&2
  return 1
}

jmap() {
  local pw
  pw=$(cat "$DIR/admin-password")
  curl -sf -u "admin:$pw" -H 'Content-Type: application/json' \
    "http://127.0.0.1:$RECOVERY_PORT/jmap/" \
    -d "{\"using\":[\"urn:ietf:params:jmap:core\",\"urn:stalwart:jmap\"],\"methodCalls\":$1}"
}

cmd_install() {
  [[ -x "$BIN" ]] && { echo "Stalwart ist schon installiert."; return; }
  local tmp
  tmp=$(mktemp -d)
  curl -sSLo "$tmp/stalwart.tar.gz" \
    "https://github.com/stalwartlabs/stalwart/releases/download/$VERSION/stalwart-aarch64-apple-darwin.tar.gz"
  mkdir -p "$DIR/bin"
  tar -xzf "$tmp/stalwart.tar.gz" -C "$DIR/bin"
  chmod +x "$BIN"
  rm -rf "$tmp"
  echo "Stalwart $VERSION installiert."
}

cmd_start() {
  [[ -x "$BIN" ]] || { echo "Erst: $0 install" >&2; exit 1; }
  [[ -f "$CONFIG" ]] || { echo "Erst: $0 reset" >&2; exit 1; }
  running && { echo "Läuft bereits."; return; }
  if nc -z 127.0.0.1 1143 2>/dev/null; then
    echo "Port 1143 ist schon belegt (anderer Prozess?): $(lsof -nP -iTCP:1143 -sTCP:LISTEN | tail -1)" >&2
    exit 1
  fi
  nohup "$BIN" --config="$CONFIG" >"$LOG" 2>&1 &
  wait_port 1143 && wait_port 1587
  echo "Testserver läuft (IMAP 1143, SMTP 1587, JMAP 18080)."
}

cmd_stop() {
  pkill -f "stalwart --config=$CONFIG" 2>/dev/null || true
  for _ in $(seq 1 25); do running || break; sleep 0.2; done
  echo "Testserver gestoppt."
}

cmd_reset() {
  [[ -x "$BIN" ]] || cmd_install
  cmd_stop
  rm -rf "$DIR/data"
  mkdir -p "$DIR/data"
  printf '{"@type":"RocksDb","path":"%s/data"}\n' "$DIR" >"$CONFIG"
  openssl rand -hex 16 >"$DIR/admin-password"
  chmod 600 "$DIR/admin-password"

  # Einrichtungsmodus: Stalwart v1 speichert alle Einstellungen in seiner
  # Datenbank, deshalb wird über die Verwaltungs-API konfiguriert.
  STALWART_RECOVERY_MODE=1 \
    STALWART_RECOVERY_ADMIN="admin:$(cat "$DIR/admin-password")" \
    STALWART_RECOVERY_MODE_PORT=$RECOVERY_PORT \
    nohup "$BIN" --config="$CONFIG" >"$LOG" 2>&1 &
  wait_port $RECOVERY_PORT

  local domain_id
  domain_id=$(jmap '[["x:Domain/set",{"create":{"d1":{"name":"kolibri.test"}}},"a"]]' |
    python3 -c 'import json,sys; print(json.load(sys.stdin)["methodResponses"][0][1]["created"]["d1"]["id"])')

  jmap '[
    ["x:NetworkListener/set",{"create":{
      "l1":{"name":"imap-test","protocol":"imap","bind":{"127.0.0.1:1143":true},"useTls":false},
      "l2":{"name":"submission-test","protocol":"smtp","bind":{"127.0.0.1:1587":true},"useTls":false},
      "l3":{"name":"http-test","protocol":"http","bind":{"127.0.0.1:18080":true},"useTls":false}}},"a"],
    ["x:Imap/set",{"update":{"singleton":{"allowPlainTextAuth":true,"rateLimit":{"count":1000000,"period":60000}}}},"b"],
    ["x:MtaStageAuth/set",{"update":{"singleton":{"saslMechanisms":{"match":{},"else":"[plain, login]"}}}},"c"],
    ["x:Account/set",{"create":{"u1":{"@type":"User","name":"test","domainId":"'"$domain_id"'",
      "credentials":{"0":{"@type":"Password","secret":"kolibri-test"}},"roles":{"@type":"User"}}}},"d"]
  ]' >/dev/null

  cmd_stop
  cmd_start
  echo "Frisch eingerichtet: test@kolibri.test / kolibri-test"
}

case "${1:-}" in
  install) cmd_install ;;
  start) cmd_start ;;
  stop) cmd_stop ;;
  reset) cmd_reset ;;
  status) running && echo "läuft" || echo "gestoppt" ;;
  *) sed -n '2,15p' "$0"; exit 1 ;;
esac
