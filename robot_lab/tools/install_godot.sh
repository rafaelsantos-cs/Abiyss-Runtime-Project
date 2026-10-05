#!/bin/sh
# Download the exact Godot build the lab was validated with and verify its
# SHA-512 against the official release checksums before installing.
#   robot_lab/tools/install_godot.sh [install_dir]   (default: $HOME/.local/godot)
set -eu
VERSION="4.7.2-stable"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) ASSET="Godot_v${VERSION}_linux.x86_64" ;;
  aarch64|arm64) ASSET="Godot_v${VERSION}_linux.arm64" ;;
  *) echo "unsupported architecture: $ARCH" >&2; exit 2 ;;
esac
DEST="${1:-$HOME/.local/godot}"
BASE="https://github.com/godotengine/godot/releases/download/${VERSION}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
curl -fsSL -o "$TMP/${ASSET}.zip" "$BASE/${ASSET}.zip"
curl -fsSL -o "$TMP/SHA512-SUMS.txt" "$BASE/SHA512-SUMS.txt"
EXPECTED="$(grep " ${ASSET}.zip\$" "$TMP/SHA512-SUMS.txt" | cut -d' ' -f1)"
ACTUAL="$(sha512sum "$TMP/${ASSET}.zip" | cut -d' ' -f1)"
if [ -z "$EXPECTED" ] || [ "$EXPECTED" != "$ACTUAL" ]; then
  echo "checksum mismatch for ${ASSET}.zip" >&2
  exit 1
fi
mkdir -p "$DEST"
unzip -o -q "$TMP/${ASSET}.zip" -d "$DEST"
ln -sf "$DEST/${ASSET}" "$DEST/godot"
"$DEST/godot" --version
echo "installed: $DEST/godot  (export ABIYSS_GODOT=$DEST/godot)"
