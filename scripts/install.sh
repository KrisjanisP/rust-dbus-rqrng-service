#! /bin/bash

set -ex # exit on error, print commands

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# 1) Build the binary
cargo build --release

# 2) Place binary at a well-known path (hard link to ~/.local/bin)
mkdir -p ~/.local/bin
ln -f target/release/trngdbus ~/.local/bin/trngdbus

# 3) Create the D-Bus activation file pointing to that path
mkdir -p ~/.local/share/dbus-1/services
cat > ~/.local/share/dbus-1/services/lv.lumii.qrng.service <<EOF
[D-BUS Service]
Name=lv.lumii.qrng
Exec=$HOME/.local/bin/trngdbus
EOF

# 4) Reload D-Bus service files (no logout needed)
busctl --user call org.freedesktop.DBus / org.freedesktop.DBus ReloadConfig

# 5) Test the service

