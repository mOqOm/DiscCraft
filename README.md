# DiscCraft

English | [日本語](README.ja.md)

An open-source DVD / Blu-ray authoring app written in pure Rust, aiming for feature parity with Adobe Encore.

## Overview

DiscCraft is an authoring app that turns video, audio and still images into DVD-Video or Blu-ray (BDMV). You assemble menus,
chapters, multiple audio tracks and subtitles, and navigation, transcode to whatever the spec requires, and write out a folder,
an ISO image, or a disc. It is also the way out of FilmCraft: turn an edited project into a disc.

Goals:

- A layout and workflow that people coming from Encore will not have to think about
- Spec-compliant DVD-Video that plays on standalone players. Blu-ray (HDMV) next
- Pure Rust, clean-room implementation, Windows / macOS / Linux
- Every action is a command. The same actions are available from the GUI, the CLI, the JSON control channel and the MCP server
- No crashes. Never lose work

Non-goals:

- Copy protection (CSS, AACS, and the like), because they require a license. Only setting the region-code flags is supported
- BD-J (Java) menus. Blu-ray is HDMV only
- Encoding DTS, Dolby TrueHD, DTS-HD
- Video editing. Leave that to FilmCraft and interoperate with it

## Status

In development (0.1.0). What works now:

- From MP4 / MOV, a camcorder's DV and other source files you can build a DVD with menus (`VIDEO_TS` folder, ISO) and a Blu-ray (BDMV folder, ISO).
  Sources that do not fit the spec are converted automatically during the build (in `disccraft-cache` next to the output by
  default; `build --cache <folder>` keeps them in a shared folder outside the output).
- The GUI (`cargo run --release -p disccraft`) uses Encore's arrangement and menu bar: the Project / Menu / Timeline / Build tabs,
  the monitor, menu viewer and flowchart, the properties panel, the layers / library panels, and the timeline viewer. Tabs can be
  dragged to reorder, move, split and float, closed with × and reopened from the window menu. Arrangements can be saved and named
  as workspaces. The Project panel's right-click menu is laid out as Encore's (rows not implemented yet are grey).
- A video hours long and tens of gigabytes in size can be used right after it is imported: the file is opened in the background,
  and the length and size of an H.264 mkv are read from its header. The monitor's picture is decoded in the background, so the
  window does not stop while the playhead is dragged.
- Menus: button placement (multi-select, align, distribute, arrange, copy and paste), links (pick whip), default buttons,
  auto-activate, backgrounds (colour / image / video) and menu audio, end action and duration, chapter index, library presets
  (multi-page chapter templates too) and library items from PSD and picture files or folders, image and shape layers (drop
  shadow, flip, rotate), guides, vertical text, creating a text subpicture.
- Timeline: one title from several video clips butted together (a dropped clip goes after the clips so far with a chapter at the
  join; MPEG-2 that goes on the disc as it is is trimmed at GOP starts, and sources that need converting and HD are joined at
  build time too and trimmed to the frame), chapter points, audio and subtitle tracks (a timeline's colour set picks the subtitle
  colours), end actions, remote menu, playback (Space, with audio; the speaker in the track header picks which audio track the
  monitor plays. A video whose picture cannot be decoded still plays its sound, and the monitor says why).
- Navigation: first play, title button, slideshows (a slide's name and description can become a subtitle), playlists, chapter
  playlists. File ▸ Preview lets you try the disc in the project preview window
  without building it (our own VM: remote d-pad, title / menu, audio / subtitle, chapter, pause. Titles play with video and audio
  and continue into their end action on completion. Built discs can be previewed in the monitor too).
- Every action is an engine command, so the GUI, the CLI and MCP can do the same thing. One user action undoes in one step.

## Features

What works today:

- DVD: MPEG-2 (720 / 704 / 352 wide) / MPEG-1 / LPCM (up to 96 kHz, 24-bit) / AC-3 / DTS / MPEG-1 Layer II (PAL) passthrough and
  re-encoding to MPEG-2, VIDEO_TS folders and ISO images (dual-layer layer break)
- DVD: titles (including a timeline built from several video clips), chapters, multiple audio tracks, subtitles, 16:9,
  region codes, playlists and chapter playlists
- Blu-ray: BDMV folders and ISO images, PG subtitles, IG menus (HDMV; always-on and popup menus, motion menus), user
  operations (UO masks). Video is SD (720×480 / 720×576) MPEG-2 or HD (1080p 23.976 / 24 and
  720p 59.94 / 50 / 23.976 / 24 as H.264, 1080i 59.94 / 50 as MPEG-2 or, by the disc's codec setting, interlaced H.264; titles, subtitles and menus and popups are at the HD size too.
  Sources at 29.97 / 25 fps become HD at their own speed as 1080i 59.94 / 50. H.264 in an `.m2ts` / `.mkv` / `.mp4` / `.mov` that fits a
  Blu-ray, interlaced 1080i included, goes on the disc as it is, without conversion. An mkv / mp4 / mov is written to the disc
  straight from the source file, so no working space the size of the video is needed). Audio goes on as it is as LPCM (up to
  7.1 and 192 kHz, 24-bit), AC-3 or DTS
- Menus: still menus, buttons and highlights, motion menus, timeouts (duration and end action), auto-activate, PSD import with
  button detection from layer names, layered PSD export with editable text, chapter index
- Sources: video (MP4 / MOV / MKV / m2v / DV and more), audio (WAV / AC-3 / MP3 / FLAC and more), images (PNG / JPEG / PSD / TIFF),
  vector graphics (SVG, drawn at the displayed size so they never blur), subtitles (SRT / WebVTT / timecode text;
  UTF-8 / UTF-16 / Shift_JIS)
- Navigation: first play, title button, remote menu, links, flowchart, playlists, slideshows
- Blu-ray to DVD: File ▸ Import ▸ Blu-ray… makes a timeline of an unencrypted Blu-ray's playlist (its clips, chapters and
  LPCM / AC-3 sound) and its menu a motion menu with the same buttons linked to the same chapters, ready to build as
  a DVD; `bd-chapters` on the command line prints a Blu-ray's chapters
- Writing to disc (Windows): the Build panel's "DVD Disc / Blu-ray Disc" output writes to a recorder, with the write speed and the number of copies (erasing rewritable discs too); also `recorders` and `burn` on the command line
- Transcode: automatic transcoding (audio to Dolby Digital (AC-3) at 192 kbps), bitrate allocation to fit the capacity, caching,
  several sources converted in parallel (the MPEG-2 encoder is multithreaded over macroblock rows, AC-3 is encoded in segments
  on every core). Transcode Now in the Project panel's right-click menu converts media in the background ahead of the build.
  Encore's quality presets (VBR 1-pass / 2-pass, CBR) and presets of your own, transcode settings per asset and Revert to
  Original, 3:2 pulldown for 23.976 sources, and on Windows HD H.264 on the hardware encoder (Preferences)
- Output: DVD-5 / DVD-9, BD-25 / BD-50, BDXL (100 GB triple layer, 128 GB quad layer), capacity meter, DVD-ROM / BD-ROM content (files for a computer beside the video)
- Verification: project checks (results grouped per item, in Encore's wording), the VM simulator and preview, read-back
  verification

## Requirements

Rust stable 1.95 or newer (rustup). Windows comes first; macOS and Linux are also targets. On Linux, playback audio (cpal) needs
the ALSA development packages (`libasound2-dev` on Debian / Ubuntu, `alsa-lib-devel` on Fedora).

The releases carry packages built on GitHub's runners:

| System | Files |
|---|---|
| Windows (x64, Arm64) | `.msi` installer, `-portable.zip` (nothing installed) |
| macOS (Apple silicon and Intel in one) | `.dmg` (DiscCraft.app), `disccraft-cli-…-macos-universal.zip` (the command line) |
| Linux (x86_64, aarch64) | `.AppImage` (with `.zsync` for updates), `.flatpak`, `.deb`, `.rpm`, `.pkg.tar.zst` (Arch), `.tar.gz` |
| Linux (riscv64) | `.deb`, `.rpm`, `.tar.gz` |

**Only the Windows x64 build is verified.** The others are built and packaged automatically and nothing more; nobody has
opened their window or checked a disc they made. Nothing is signed with a developer certificate: Windows SmartScreen asks
before the installer first runs, and macOS Gatekeeper stops the app the first time (open it from the right-click menu, or
remove the mark with `xattr -d com.apple.quarantine`). The Linux packages need ALSA (`libasound2`) and the X11 or Wayland
libraries; the AppImage starts the command line with `disccraft-cli` as its first argument. Reports of what works and what
does not are welcome.

## Building and usage

```
cargo build --release
```

This produces the GUI (`disccraft`) and the CLI (`disccraft-cli`) in `target/release`.

For the GUI, launch `disccraft` and create a new project from the welcome screen or the File menu. Import assets, create
timelines and menus and link them, then write the disc to a folder or ISO image from the Build panel.

The CLI builds a project by running commands (JSON, one per line) in order, then builds it:

```
disccraft-cli commands                           # list commands
disccraft-cli run project.dcproj commands.jsonl  # run commands in order (creates the project if missing)
disccraft-cli inspect project.dcproj             # show the project and check results
disccraft-cli build project.dcproj --out out     # write a VIDEO_TS / BDMV folder
disccraft-cli build project.dcproj --iso disc.iso
```

Example `commands.jsonl` (a DVD with one video and one menu):

```
{"command": "file.new", "params": {"kind": "dvd", "standard": "ntsc"}}
{"command": "asset.import", "params": {"path": "movie.mp4"}}
{"command": "timeline.create", "params": {"asset": 1, "name": "Main feature"}}
{"command": "menu.create", "params": {"name": "Main"}}
{"command": "menu.addButton", "params": {"menu": 3, "label": "Play", "target": {"timeline": 2}}}
{"command": "timeline.setEndAction", "params": {"timeline": 2, "target": "lastMenu"}}
{"command": "nav.setFirstPlay", "params": {"target": {"menu": 3}}}
```

Material that doesn't meet the spec is converted at build time and kept in `disccraft-cache` in the output folder (later
builds skip the conversion).

The [user manual](docs/manual/en/README.md) explains how to use it. What is not done yet is in [open issues](docs/open-issues.md). What changed in each version is in the [changelog](CHANGELOG.md).

## Driving it from an AI agent (MCP)

`disccraft-cli mcp` runs as an MCP server (JSON-RPC over stdio). There are three tools: `disccraft_commands` (the command list),
`disccraft_execute` (run an engine command) and `disccraft_inspect` (the project and the check results).

In your MCP client, register `<repository path>/target/release/disccraft-cli` as the command with `mcp` as its argument (connected over stdio).

## Layout

```
DiscCraft/
├─ apps/
│  ├─ disccraft/
│  └─ disccraft-cli/
├─ crates/
│  ├─ mpeg2venc/
│  ├─ ac3enc/
│  ├─ psmux/
│  ├─ spu/
│  ├─ ifo/
│  ├─ m2tsmux/
│  ├─ bdmv/
│  ├─ pgs/
│  ├─ udf/
│  ├─ project/
│  ├─ vm/
│  ├─ menu/
│  ├─ motion/
│  ├─ photo/        (PhotoCraft integration: PSD and more)
│  ├─ sound/        (SoundCraft integration)
│  ├─ nav/
│  ├─ transcode/
│  ├─ check/
│  ├─ dvd/
│  ├─ bd/
│  ├─ build/
│  ├─ engine/
│  ├─ ui-egui/
│  ├─ automation/
│  └─ platform/
└─ assets/
```

## Reusing FilmCraft

Crates from FilmCraft (https://github.com/storytold/filmcraft, MIT OR Apache-2.0) are used as git dependencies (pinned rev).
Reused crates are listed in NOTICE.

| crate | what it is used for |
|---|---|
| filmcraft-time | exact time with integer ticks |
| filmcraft-geom | coordinates and rectangles |
| filmcraft-color | colour conversion |
| filmcraft-bitstream | bit reading and writing |
| filmcraft-frame | frame representation |
| filmcraft-media | reading source media |
| filmcraft-codecs | decoding MP4 / MOV / MKV / MXF / MPEG-TS / PS and more |
| filmcraft-text | drawing text in menus |
| filmcraft-audio-dsp | loudness measurement and normalisation |
| filmcraft-project, filmcraft-format | reading a FilmCraft project (`.fcproj`): a sequence's markers become chapter points |

FilmCraft's crates build on the craft-libs crates (https://github.com/storytold/craft-libs, MIT OR Apache-2.0) for things such as
video and audio decoding. DiscCraft uses craft-libs' craft-audio (the AC-3 encoder that turns converted audio into Dolby Digital),
craft-video (the H.264 encoder for Blu-ray video, which used to be FilmCraft's filmcraft-h264enc; in tests also its MPEG-2 decoder,
verifying the MPEG-2 encoder's output), craft-media (reading MPEG-2 program and transport streams, Matroska and MP4, to take out video and audio that fit the format
without converting them), craft-ui (panel and workspace docking), craft-captions (subtitle files in ASS / SSA, SCC, MCC, EBU STL,
TTML / DFXP and SAMI), craft-mcp-server (the MCP server of `disccraft-cli mcp`), craft-sniff (an imported file's kind from its first bytes),
craft-settings (the settings file, `config.toml`), craft-dialogs (native file dialogs that never hold up a frame), craft-update (Help ▸ Check for Updates…: finding, downloading and installing a newer release) and craft-fonts-build
(the build embeds craft-fonts' faces, which craft-ui's font stack puts under Encore's), and in tests also craft-psd (writing PSD files
for tests) and craft-media-testkit (finding ffmpeg and ffprobe for the tests that compare with them). Release builds embed fonts from craft-fonts (https://github.com/storytold/craft-fonts, OFL-1.1) for the scripts the
interface's own faces lack (Arabic, Thai, Hebrew, symbols and others); NOTICE lists them with their licence.

## Trademarks

"DVD" and "Blu-ray Disc" are used descriptively only, and no logos are used. Adobe Encore is a trademark of Adobe, and DiscCraft
is not affiliated with Adobe.

## License

Dual-licensed under MIT OR Apache-2.0. See LICENSE-MIT and LICENSE-APACHE for details. Unless stated otherwise, contributions to
this repository are offered under the same dual license.
