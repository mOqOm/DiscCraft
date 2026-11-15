# DiscCraft

[日本語](README.md) | **English**

Open-source DVD / Blu-ray authoring software written in pure Rust, aiming for feature parity with Adobe Encore.

## Overview

DiscCraft is authoring software that turns video, audio and still images into DVD-Video and Blu-ray (BDMV) discs. It assembles menus, chapters, multiple audio and subtitle tracks, and navigation, transcodes material to meet the disc specifications, and writes the result to a folder, an ISO image or a disc. It also serves as the way to put work edited in FilmCraft onto disc.

Goals:

- A layout and workflow that feel familiar to anyone who used Encore
- Spec-compliant DVD-Video that plays on home players, followed by Blu-ray (HDMV)
- Pure Rust, clean-room implementation, Windows / macOS / Linux
- Every operation is a command: the GUI, CLI, JSON control channel and MCP server can all do the same things
- No crashes, no lost work

Non-goals:

- Copy protection (CSS, AACS, etc.), since it requires licensing. Only the region code flags are supported
- BD-J (Java) menus. Blu-ray support is HDMV only
- DTS, Dolby TrueHD and DTS-HD encoding
- Video editing. Editing is left to FilmCraft, which DiscCraft works with

## Status

In development (0.1.0). What works today:

- Build DVDs (`VIDEO_TS` folder, ISO) and Blu-rays (BDMV folder, ISO) with menus from MP4 / MOV and other sources.
  Material that doesn't meet the spec is converted automatically at build time (stored in `disccraft-cache` in the output folder).
- The GUI (`cargo run --release -p disccraft`) uses the same layout and menu bar as Encore: Project / Menus / Timelines / Build tabs,
  Monitor, Menu Viewer, Flowchart, Properties, Layers / Library and the Timeline Viewer. Tabs can be dragged to reorder, move, split
  or float; close them with × and reopen them from the Window menu. Layouts can be saved as named workspaces.
- Menus: button placement (multi-select, align, distribute, arrange, copy and paste), links (pick whip), default button,
  auto-activate, backgrounds (color / image / video) and menu audio, end action and duration, chapter indexes, library presets.
- Timelines: chapter points, audio / subtitle tracks, end actions, remote menus, playback (Space, with sound).
- Navigation: first play, title button, slideshows, playlists. File ▸ Preview lets you try the disc in the Project Preview window
  without building it (DiscCraft's own VM: remote-control arrow keys, title / menu, audio / subtitles, chapters, pause.
  Titles play with picture and sound and then go to their end action. Built discs can also be previewed in the Monitor).
- Every operation is an engine command, available identically from the GUI, CLI and MCP. Each operation can be undone in one step.

## Features

- DVD: MPEG-2 / LPCM / AC-3 passthrough and re-encoding to MPEG-2, VIDEO_TS folders and ISO images (dual-layer layer break)
- DVD: titles, chapters, multiple audio tracks, SRT subtitles, 16:9, region codes
- Blu-ray: BDMV folders and ISO images, PG subtitles, IG menus (HDMV; always-on / pop-up, motion menus),
  user operations (UO masks). Video is SD (720×480 / 720×576) MPEG-2 or HD (1080p 23.976 / 24 / 25 / 29.97 /
  50 / 59.94, 720p 59.94 / 50 / 23.976 / 24) H.264 (titles, subtitles, menus and pop-ups are HD-sized too. 1080p at 25 / 29.97 /
  50 / 59.94 is outside the Blu-ray spec and is planned for removal)
- Menus: still menus, buttons and highlights, motion menus, timeouts (duration and end action),
  auto-activate, PSD import with button detection from layer names, chapter indexes
- Assets: video (MP4 / MOV / MKV / m2v, etc.), audio (WAV / AC-3 / MP3 / FLAC, etc.), images (PNG / JPEG / PSD / TIFF),
  vector images (SVG, rendered at display size so they stay sharp), subtitles (SRT / WebVTT / timecoded text; UTF-8 / UTF-16 / Shift_JIS)
- Navigation: first play, title button, remote menus, links, flowchart, playlists, slideshows
- Transcoding: automatic transcoding (audio to Dolby Digital (AC-3) at 192 kbps), bitrate allocation to fit the disc, caching
- Output: DVD-5 / DVD-9, BD-25 / BD-50, BDXL (100 GB triple-layer, 128 GB single-sided quad-layer), capacity meter
- Verification: project check, VM simulator and preview, read-back verification

## Requirements

Rust stable 1.95 or later (rustup). Windows is the top priority; macOS and Linux are also supported. On Linux, playback audio (cpal)
needs the ALSA development package (`libasound2-dev` on Debian / Ubuntu, `alsa-lib-devel` on Fedora).

## Building and usage

```
cargo build --release
```

This produces the GUI (`disccraft`) and the CLI (`disccraft-cli`) in `target/release`.

For the GUI, launch `disccraft` and create a new project from the welcome screen or the File menu. Import assets, create timelines
and menus and link them, then write the disc to a folder or ISO image from the Build panel.

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

Material that doesn't meet the spec is converted at build time and kept in `disccraft-cache` in the output folder (later builds skip the conversion).

## Using from AI agents (MCP)

`disccraft-cli mcp` runs as an MCP server (JSON-RPC over stdio). It provides three tools: `disccraft_commands` (list commands),
`disccraft_execute` (run an engine command) and `disccraft_inspect` (the project and check results).

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
│  ├─ photo/        (PhotoCraft integration: PSD, etc.)
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

## Reuse of FilmCraft

DiscCraft uses crates from FilmCraft (https://github.com/storytold/filmcraft, MIT OR Apache-2.0) as git dependencies (pinned to a rev). Reused crates are listed in NOTICE.

| crate | Used for |
|---|---|
| filmcraft-time | Exact time in integer ticks |
| filmcraft-geom | Coordinates and rectangles |
| filmcraft-color | Color conversion |
| filmcraft-bitstream | Bit reading and writing |
| filmcraft-frame | Frame representation |
| filmcraft-media | Asset loading |
| filmcraft-codecs | Decoding MP4 / MOV / MKV / MXF / MPEG-TS / PS, etc. |
| filmcraft-h264enc | Blu-ray video encoding |
| filmcraft-text | Menu text rendering |
| filmcraft-audio-dsp | Loudness measurement and normalization |

The FilmCraft crates use crates from craft-libs (https://github.com/storytold/craft-libs, MIT OR Apache-2.0), for example for video and audio decoding. DiscCraft itself uses craft-audio (the AC-3 encoder that turns converted audio into Dolby Digital) and craft-ui (panel and workspace docking) from craft-libs, and its tests also use craft-video (to verify the MPEG-2 encoder's output) and craft-psd (to write PSD files for tests).

## Trademarks

"DVD" and "Blu-ray Disc" are used descriptively only; no logos are used. Adobe Encore is a trademark of Adobe, and DiscCraft is not affiliated with Adobe.

## License

Dual-licensed under MIT OR Apache-2.0. See LICENSE-MIT and LICENSE-APACHE for details. Unless explicitly agreed otherwise, contributions to this repository are provided under the same dual license.
