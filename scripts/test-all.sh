#!/usr/bin/env bash
# Alle Tests: Unit-Tests, dann Servertests gegen einen frisch eingerichteten
# lokalen Stalwart (Reihenfolge fest, weil einige Tests Daten verändern).
#
#   scripts/test-all.sh          # alles
#   scripts/test-all.sh --unit   # nur Unit-Tests (ohne Server)
set -euo pipefail
cd "$(dirname "$0")/.."
source "$HOME/.cargo/env" 2>/dev/null || true

echo "== Unit-Tests"
cargo test --workspace --quiet

[[ "${1:-}" == "--unit" ]] && exit 0

echo "== Testserver frisch einrichten"
scripts/test-server.sh reset
python3 -I scripts/seed-mail.py

export KOLIBRI_TEST_SERVER=1
run() { echo "== $*"; cargo test --quiet "$@" -- --include-ignored --test-threads=1; }

# Nur lesend zuerst, dann die verändernden Tests.
run -p kolibri-mail --test live_server
run -p kolibri --lib sync::
run -p kolibri --lib accounts::
run -p kolibri --lib keychain::
run -p kolibri --lib queue::server_tests::queue_actions_against_server
run -p kolibri --lib compose::server_tests
run -p kolibri --lib contacts::server_tests
run -p kolibri --lib calendar::server_tests
run -p kolibri-dav --test live_carddav
run -p kolibri --lib queue::server_tests::queue_survives_offline

scripts/test-server.sh stop
echo "== Alles grün."
