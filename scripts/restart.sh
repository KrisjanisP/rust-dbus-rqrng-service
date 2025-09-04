#!/bin/bash

set -ex # exit on error, print commands

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# 1) Recompile the binary
cargo build --release

# 2) Update the binary at the well-known path (hard link to ~/.local/bin)
mkdir -p ~/.local/bin
ln -f target/release/trngdbus ~/.local/bin/trngdbus

# 3) Find out the PID and kill the running service
OUT=$(busctl --user status lv.lumii.trng 2>/dev/null || true)
export PID=$(echo "$OUT" | grep "^PID=" | cut -c 5- || true)
if [ -n "$PID" ]; then
  echo "Killing existing service with PID: $PID"
  kill $PID
fi
