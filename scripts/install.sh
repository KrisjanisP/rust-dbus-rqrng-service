#! /bin/bash

set -ex # exit on error, print commands

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# 1) Build the binary
cargo build --release

# 2) Place binary at a well-known path (hard link to ~/.local/bin)
mkdir -p ~/.local/bin
ln -f target/release/trngdbus ~/.local/bin/trngdbus

# 3) Copy config file to user config directory
mkdir -p ~/.config/trng-dbus
cp docs/example.toml ~/.config/trng-dbus/config.toml

# 4) Create the D-Bus activation file pointing to that path
mkdir -p ~/.local/share/dbus-1/services
cat > ~/.local/share/dbus-1/services/lv.lumii.trng.service <<EOF
[D-BUS Service]
Name=lv.lumii.trng
Exec=$HOME/.local/bin/trngdbus
EOF

# 5) Reload D-Bus service files (no logout needed)
busctl --user call org.freedesktop.DBus / org.freedesktop.DBus ReloadConfig

# 6) Test the service
echo "Testing service with busctl..."
busctl --user call \
  lv.lumii.trng \
  /lv/lumii/trng/SourceXorAggregator \
  lv.lumii.trng.Rng \
  ReadBytes \
  tt 16 1000

echo "Installation complete! Service should auto-start on D-Bus requests."
