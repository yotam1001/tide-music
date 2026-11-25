#!/bin/sh
# Install or update Tide on a Mac from the latest GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/yotam1001/tide-music/main/scripts/install-tide-macos.sh | sh
#
# Tide is ad-hoc signed, so a disk image downloaded in a browser is blocked by
# Gatekeeper until it is approved in System Settings. This script downloads
# the image for your processor with curl, checks it against the SHA-256
# published with the release, verifies Tide's code signature and identity,
# and copies Tide into Applications. Your settings and session are kept.
set -eu

REPO="yotam1001/tide-music"
BUNDLE_ID="com.tidemusic.Tide"

case "$(uname -m)" in
  arm64) IMAGE="Tide.dmg" ;;
  x86_64) IMAGE="Tide-Intel.dmg" ;;
  *) echo "Tide supports Apple Silicon and Intel Macs only." >&2; exit 1 ;;
esac
major="$(sw_vers -productVersion | cut -d. -f1)"
if [ "$major" -lt 14 ]; then
  echo "Tide needs macOS 14 Sonoma or later." >&2
  exit 1
fi

if [ -w /Applications ]; then
  DEST_DIR="/Applications"
else
  DEST_DIR="$HOME/Applications"
  mkdir -p "$DEST_DIR"
fi
DEST="$DEST_DIR/Tide.app"

work="$(mktemp -d "${TMPDIR:-/tmp}/tide-install.XXXXXX")"
mount="$work/mount"
cleanup() {
  hdiutil detach -quiet -force "$mount" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT INT TERM

base="https://github.com/$REPO/releases/latest/download"
echo "Downloading $IMAGE…"
curl -fL --proto '=https' --tlsv1.2 --progress-bar -o "$work/$IMAGE" "$base/$IMAGE"
curl -fsSL --proto '=https' --tlsv1.2 -o "$work/$IMAGE.sha256" "$base/$IMAGE.sha256"

expected="$(awk 'NF { print tolower($1); exit }' "$work/$IMAGE.sha256")"
named="$(awk 'NF { sub(/^\*/, "", $2); print $2; exit }' "$work/$IMAGE.sha256")"
actual="$(shasum -a 256 "$work/$IMAGE" | awk '{ print $1 }')"
if [ "${#expected}" -ne 64 ] || [ "$named" != "$IMAGE" ] || [ "$expected" != "$actual" ]; then
  echo "The download did not match its published checksum. Nothing was installed." >&2
  exit 1
fi
echo "Checksum verified."

mkdir "$mount"
hdiutil attach -nobrowse -readonly -noautoopen -quiet -mountpoint "$mount" "$work/$IMAGE"
source_app="$mount/Tide.app"
id="$(plutil -extract CFBundleIdentifier raw -o - "$source_app/Contents/Info.plist")"
version="$(plutil -extract CFBundleShortVersionString raw -o - "$source_app/Contents/Info.plist")"
if [ "$id" != "$BUNDLE_ID" ]; then
  echo "The disk image does not contain Tide. Nothing was installed." >&2
  exit 1
fi
codesign --verify --deep --strict "$source_app"

ditto --noqtn "$source_app" "$work/Tide.app"
hdiutil detach -quiet -force "$mount" >/dev/null 2>&1 || true

if pgrep -x tide-music >/dev/null 2>&1; then
  echo "Quitting Tide…"
  osascript -e "tell application id \"$BUNDLE_ID\" to quit" >/dev/null 2>&1 || true
  i=0
  while pgrep -x tide-music >/dev/null 2>&1 && [ "$i" -lt 50 ]; do
    sleep 0.2; i=$((i + 1))
  done
  pkill -x tide-music >/dev/null 2>&1 || true
fi

if [ -e "$DEST" ]; then
  rm -rf "$work/previous.app"
  mv "$DEST" "$work/previous.app"
fi
if ! mv "$work/Tide.app" "$DEST"; then
  [ -e "$work/previous.app" ] && mv "$work/previous.app" "$DEST"
  echo "Could not copy Tide to $DEST_DIR." >&2
  exit 1
fi

echo "Tide $version is installed in $DEST_DIR."
open "$DEST"
