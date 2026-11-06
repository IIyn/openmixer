#!/bin/sh
# Installs the OpenMixer icon and launcher for the current user.
# Usage: packaging/install-desktop.sh [path/to/openmixer]   (default: target/release/openmixer)
set -eu

here=$(cd "$(dirname "$0")" && pwd)
bin=${1:-"$here/../target/release/openmixer"}
if [ ! -x "$bin" ]; then
    echo "binary not found: $bin (run 'cargo build --release' first)" >&2
    exit 1
fi
bin=$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")

data=${XDG_DATA_HOME:-"$HOME/.local/share"}
icons="$data/icons/hicolor"

install -Dm644 "$here/openmixer.svg" "$icons/scalable/apps/openmixer.svg"
for size in 16 24 32 48 64 128 256; do
    if command -v rsvg-convert >/dev/null 2>&1; then
        mkdir -p "$icons/${size}x${size}/apps"
        rsvg-convert -w "$size" -h "$size" "$here/openmixer.svg" -o "$icons/${size}x${size}/apps/openmixer.png"
    fi
done

mkdir -p "$data/applications"
sed "s|^Exec=.*|Exec=$bin|" "$here/openmixer.desktop" > "$data/applications/openmixer.desktop"

command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$icons" || true
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$data/applications" || true
command -v kbuildsycoca6 >/dev/null 2>&1 && kbuildsycoca6 >/dev/null 2>&1 || true

echo "Installed launcher: $data/applications/openmixer.desktop -> $bin"
