#!/usr/bin/env bash
# Makes the .flatpak bundle of DiscCraft from the Linux build's installed tree (.github/workflows/release.yml runs
# it after packaging/linux/package.sh):
#
#   packaging/linux/flatpak.sh dist/disccraft-0.1.0-alpha.6-linux-x86_64-stage.tar.gz dist
#
# Needs flatpak and flatpak-builder; the runtime and SDK come from Flathub.
set -euo pipefail

stage_tar="${1:?stage tarball}"
out="${2:-dist}"
APP_ID=io.github.mOqOm.DiscCraft
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"
base="$(basename "$stage_tar" -stage.tar.gz)"

cp "$here/flatpak/$APP_ID.yml" "$work/"
mkdir "$work/stage"
tar -C "$work/stage" -xzf "$stage_tar"

flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
build() {
  flatpak-builder --user --install-deps-from=flathub --disable-rofiles-fuse --force-clean \
    --repo="$work/repo" "$work/build" "$1"
}
if ! build "$work/$APP_ID.yml"; then
  # appstreamcli compose failed on the x86_64 runner alone ("file-read-error", 2026-11-19) with the files that
  # compose on aarch64. The bundle is then made without the composed catalogue (the metainfo file is still in
  # it), and the log says so.
  echo "::warning::appstream compose failed; building the Flatpak without it"
  sed 's/^command: disccraft$/command: disccraft\nappstream-compose: false/' "$work/$APP_ID.yml" \
    > "$work/$APP_ID.nocompose.yml"
  grep -q '^appstream-compose: false$' "$work/$APP_ID.nocompose.yml"
  build "$work/$APP_ID.nocompose.yml"
fi
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
  "$work/repo" "$out/$base.flatpak" "$APP_ID"
ls -l "$out/$base.flatpak"
