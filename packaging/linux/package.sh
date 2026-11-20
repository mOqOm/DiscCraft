#!/usr/bin/env bash
# Packages the Linux build of DiscCraft (.github/workflows/release.yml runs it).
#
#   VERSION=0.1.0-alpha.6 ARCH=x86_64 BIN_DIR=target/release FORMATS="tar deb rpm arch appimage" \
#     packaging/linux/package.sh
#
# - VERSION: the release without its "v" ("test" in a dry run).
# - ARCH: x86_64, aarch64 or riscv64.
# - BIN_DIR: where disccraft and disccraft-cli are.
# - FORMATS: any of tar (.tar.gz), deb, rpm, arch (.pkg.tar.zst), appimage (.AppImage and its .zsync).
# - OUT: where the packages go (default dist).
#
# Also writes $OUT/disccraft-$VERSION-linux-$ARCH-stage.tar.gz, the installed tree the Flatpak job repacks. Needs
# nfpm (deb, rpm, arch) and appimagetool with its runtime (appimage; APPIMAGETOOL, APPIMAGE_RUNTIME), zsyncmake for
# the .zsync. The packages' library dependencies are read from the programs (readelf): a library the table below
# does not know stops the script, so that no package goes out without it.
set -euo pipefail

: "${VERSION:?}" "${ARCH:?}" "${BIN_DIR:?}"
FORMATS="${FORMATS:-tar deb rpm arch appimage}"
OUT="${OUT:-dist}"
APP_ID=io.github.mOqOm.DiscCraft
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
base="disccraft-$VERSION-linux-$ARCH"

# The installed tree: programs, desktop entry, AppStream metadata, MIME type, icons, documents.
stage="$work/stage"
install -Dm755 "$BIN_DIR/disccraft" "$stage/usr/bin/disccraft"
install -Dm755 "$BIN_DIR/disccraft-cli" "$stage/usr/bin/disccraft-cli"
install -Dm644 "$here/$APP_ID.desktop" "$stage/usr/share/applications/$APP_ID.desktop"
install -d "$stage/usr/share/metainfo"
sed -e "s/@VERSION@/$VERSION/" -e "s/@DATE@/$(date -u +%Y-%m-%d)/" "$here/$APP_ID.metainfo.xml.in" \
  > "$stage/usr/share/metainfo/$APP_ID.metainfo.xml"
install -Dm644 "$here/$APP_ID.mime.xml" "$stage/usr/share/mime/packages/$APP_ID.xml"
install -d "$stage/usr/share/icons"
cp -r "$root/assets/app-icon/hicolor" "$stage/usr/share/icons/hicolor"
for doc in README.md README.ja.md LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md; do
  install -Dm644 "$root/$doc" "$stage/usr/share/doc/disccraft/$doc"
done
tar -C "$stage" -czf "$OUT/$base-stage.tar.gz" usr

# The library packages each distribution names for a library the programs load (readelf's NEEDED).
deb_deps=()
rpm_deps=()
arch_deps=()
needed="$(readelf -d "$stage/usr/bin/disccraft" "$stage/usr/bin/disccraft-cli" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | sort -u)"
for lib in $needed; do
  case "$lib" in
    # The C library and its parts, the compiler's runtime: every system has them.
    libc.so.* | libm.so.* | libdl.so.* | libpthread.so.* | librt.so.* | ld-linux*.so.* | libgcc_s.so.*) ;;
    libasound.so.2)
      deb_deps+=("libasound2 | libasound2t64"); rpm_deps+=(alsa-lib); arch_deps+=(alsa-lib) ;;
    libfontconfig.so.1)
      deb_deps+=(libfontconfig1); rpm_deps+=(fontconfig); arch_deps+=(fontconfig) ;;
    libfreetype.so.6)
      deb_deps+=(libfreetype6); rpm_deps+=(freetype); arch_deps+=(freetype2) ;;
    libxkbcommon.so.0)
      deb_deps+=(libxkbcommon0); rpm_deps+=(libxkbcommon); arch_deps+=(libxkbcommon) ;;
    libX11.so.6)
      deb_deps+=(libx11-6); rpm_deps+=(libX11); arch_deps+=(libx11) ;;
    libxcb.so.1)
      deb_deps+=(libxcb1); rpm_deps+=(libxcb); arch_deps+=(libxcb) ;;
    libwayland-client.so.0)
      deb_deps+=(libwayland-client0); rpm_deps+=(libwayland-client); arch_deps+=(wayland) ;;
    libdbus-1.so.3)
      deb_deps+=(libdbus-1-3); rpm_deps+=(dbus-libs); arch_deps+=(dbus) ;;
    *)
      echo "error: $lib is loaded by the programs, but packaging/linux/package.sh does not know its package" >&2
      exit 1 ;;
  esac
done
echo "libraries: $(echo $needed)"

# deb, rpm, arch: nfpm, from a configuration written here (the dependencies differ by build).
nfpm_formats=()
for format in $FORMATS; do
  case "$format" in deb | rpm) nfpm_formats+=("$format") ;; arch) nfpm_formats+=(archlinux) ;; esac
done
if [ "${#nfpm_formats[@]}" -gt 0 ]; then
  case "$ARCH" in x86_64) goarch=amd64 ;; aarch64) goarch=arm64 ;; riscv64) goarch=riscv64 ;; *) echo "error: ARCH $ARCH" >&2; exit 1 ;; esac
  # A dry run has no version: a placeholder that every packager takes.
  pkg_version="$VERSION"
  [ "$pkg_version" = test ] && pkg_version=0.0.0
  list() { local item; for item in "$@"; do printf '      - "%s"\n' "$item"; done; }
  {
    cat <<EOF
name: disccraft
arch: $goarch
platform: linux
version: "$pkg_version"
version_schema: semver
release: "1"
section: video
priority: optional
maintainer: "mOqOm <noreply@github.com>"
description: |
  DVD and Blu-ray authoring with menus, chapters and subtitles.
  DiscCraft makes DVD-Video and Blu-ray (BDMV) discs from video, audio, images and subtitles, as folders or
  ISO images. It includes disccraft (the application) and disccraft-cli (the command line).
vendor: mOqOm
homepage: https://github.com/mOqOm/DiscCraft
license: MIT OR Apache-2.0
contents:
  - src: $stage/usr/
    dst: /usr/
    type: tree
overrides:
  deb:
    depends:
$(list "${deb_deps[@]}")
  rpm:
    depends:
$(list "${rpm_deps[@]}")
  archlinux:
    depends:
$(list "${arch_deps[@]}")
EOF
  } > "$work/nfpm.yaml"
  for packager in "${nfpm_formats[@]}"; do
    case "$packager" in deb) ext=deb ;; rpm) ext=rpm ;; archlinux) ext=pkg.tar.zst ;; esac
    nfpm package --config "$work/nfpm.yaml" --packager "$packager" --target "$OUT/$base.$ext"
  done
fi

for format in $FORMATS; do
  case "$format" in
    tar)
      # The programs and documents, with the desktop files in share/ for those who install by hand.
      folder="$work/$base"
      mkdir -p "$folder"
      cp "$stage/usr/bin/disccraft" "$stage/usr/bin/disccraft-cli" "$folder/"
      cp "$stage/usr/share/doc/disccraft/"* "$folder/"
      cp -r "$stage/usr/share" "$folder/share"
      rm -rf "$folder/share/doc"
      tar -C "$work" -czf "$OUT/$base.tar.gz" "$base"
      ;;
    appimage)
      : "${APPIMAGETOOL:?}" "${APPIMAGE_RUNTIME:?}"
      appdir="$work/DiscCraft.AppDir"
      mkdir -p "$appdir"
      cp -r "$stage/usr" "$appdir/usr"
      cp "$here/$APP_ID.desktop" "$appdir/$APP_ID.desktop"
      cp "$root/assets/app-icon/hicolor/256x256/apps/$APP_ID.png" "$appdir/$APP_ID.png"
      cp "$root/assets/app-icon/hicolor/256x256/apps/$APP_ID.png" "$appdir/.DirIcon"
      cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
# Starts DiscCraft from the AppImage; `disccraft-cli` as the first argument runs the command line instead.
here="$(dirname "$(readlink -f "$0")")"
if [ "${1:-}" = disccraft-cli ]; then
  shift
  exec "$here/usr/bin/disccraft-cli" "$@"
fi
exec "$here/usr/bin/disccraft" "$@"
EOF
      chmod 755 "$appdir/AppRun"
      # The update information makes appimagetool write the .zsync beside the AppImage.
      (cd "$OUT" && ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGETOOL" --no-appstream \
        --runtime-file "$APPIMAGE_RUNTIME" \
        -u "gh-releases-zsync|mOqOm|DiscCraft|latest|disccraft-*-linux-$ARCH.AppImage.zsync" \
        "$appdir" "$base.AppImage")
      test -f "$OUT/$base.AppImage.zsync"
      ;;
    deb | rpm | arch) ;;
    *) echo "error: unknown format $format" >&2; exit 1 ;;
  esac
done
ls -l "$OUT"
