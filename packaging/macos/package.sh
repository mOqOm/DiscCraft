#!/usr/bin/env bash
# Packages the macOS build of DiscCraft (.github/workflows/release.yml runs it):
#
#   VERSION=0.1.0-alpha.6 BIN_DIR=dist packaging/macos/package.sh
#
# - BIN_DIR: where the universal (Apple silicon and Intel) disccraft and disccraft-cli are.
# - OUT: where the packages go (default dist).
#
# Writes disccraft-$VERSION-macos-universal.dmg (DiscCraft.app, a link to Applications and the documents) and
# disccraft-cli-$VERSION-macos-universal.zip (the command line and the documents). Nothing is signed with a
# developer certificate: the app is signed ad hoc, so macOS asks before it first opens it.
set -euo pipefail

: "${VERSION:?}" "${BIN_DIR:?}"
OUT="${OUT:-dist}"
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
docs=(README.md README.ja.md LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md)

# The version macOS shows: the numbers in front of a pre-release ("0.1.0" of "0.1.0-alpha.6").
short="${VERSION%%-*}"
[ "$short" = test ] && short=0.0.0

# DiscCraft.app
app="$work/dmg/DiscCraft.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$BIN_DIR/disccraft" "$app/Contents/MacOS/disccraft"
cp "$root/assets/app-icon/disccraft.icns" "$app/Contents/Resources/disccraft.icns"
sed -e "s/@VERSION@/$VERSION/" -e "s/@SHORT_VERSION@/$short/" "$here/Info.plist.in" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

# The disk image: the app, a link to Applications to drag it onto, the documents.
ln -s /Applications "$work/dmg/Applications"
for doc in "${docs[@]}"; do cp "$root/$doc" "$work/dmg/"; done
hdiutil create -volname "DiscCraft $VERSION" -srcfolder "$work/dmg" -fs HFS+ -format UDZO -ov \
  "$OUT/disccraft-$VERSION-macos-universal.dmg"

# The command line on its own.
cli="$work/disccraft-cli-$VERSION-macos-universal"
mkdir -p "$cli"
cp "$BIN_DIR/disccraft-cli" "$cli/"
codesign --force --sign - "$cli/disccraft-cli"
for doc in "${docs[@]}"; do cp "$root/$doc" "$cli/"; done
(cd "$work" && zip -r -y "$OUT/disccraft-cli-$VERSION-macos-universal.zip" "$(basename "$cli")")
ls -l "$OUT"
