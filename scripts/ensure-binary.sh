#!/usr/bin/env bash
# Print the absolute path of a working hkbus binary.
set -euo pipefail

if [ -n "${HKBUS_BIN:-}" ] && [ -x "$HKBUS_BIN" ]; then
  printf '%s\n' "$HKBUS_BIN"
  exit 0
fi

if command -v hkbus >/dev/null 2>&1; then
  command -v hkbus
  exit 0
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [ -f "$ROOT/Cargo.toml" ] && command -v cargo >/dev/null 2>&1; then
  (cd "$ROOT" && cargo build --release --locked >/dev/null)
  BIN="$ROOT/target/release/hkbus"
  if [ -x "$BIN" ]; then
    printf '%s\n' "$BIN"
    exit 0
  fi
fi

echo "hkbus: no binary found and no Cargo build available" >&2
echo "Set HKBUS_BIN, put hkbus on PATH, or build with: cargo build --release --locked" >&2
exit 1
