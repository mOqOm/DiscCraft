English | [日本語](CHANGELOG.ja.md)

# Changelog

What changed in each version of DiscCraft. The files for each system are on [GitHub's releases](https://github.com/mOqOm/DiscCraft/releases).

## 0.1.0-alpha.13 (2026-11-24)

### Importing from Blu-ray (BD to DVD)

- **File ▸ Import ▸ Blu-ray…**: choose a playlist from a disc drive, a disc folder (one with `BDMV` in it) or a mounted disc image, and a timeline is made from it: the clips cut to the stretches that play, the chapter points, and an audio track for each sound stream (LPCM and AC-3, in the disc's languages). For turning a Blu-ray into a DVD
- The Blu-ray's menus come in too, as drafts of DVD menus (a BD and a DVD differ in how many buttons they have and where, so tidy them up after the import)
- Command line: `disccraft-cli bd-chapters <D: | disc folder | .mpls>` (the playlists and the chapters of the main one; also as `--json` or `--ogm`)

### Media that can be read

- Subtitles inside an MKV (text and PGS) come in as subtitle tracks
- A sound stream's language (an MKV's language tag, a TS / M2TS's language) becomes the audio track's language. The Blu-ray language codes cover every language
- A track that uses the second or a later sound stream shows its number in the clip's name

### Writing to discs (Windows)

- DVD+R DL (dual layer) is written with the layer change where the image's layer break is (not yet tried on a real dual-layer disc)

### Monitor and playback

- **Fast playback** (0.5× to 8×): J slower, L faster, K stop. Up to 2× the sound keeps its pitch (handy for finding where to put chapter points)
- The Monitor's controls are one bar, as in Encore. The current time can be dragged sideways a frame at a time, or clicked and typed. Home / End go to the start and the end
- Playback in the Monitor stuttered right after a project was opened, while the timeline's clip pictures were being made: fixed
- The video output's full screen is no longer covered by the taskbar

### Timeline and chapters

- Adding or moving a chapter made the button links and playlists that pointed at chapters move to another chapter: fixed
- Several chapters can be added to a chapter playlist at once
- The timeline looks like Encore's: the picture at each clip's head and chapter, file names with their extension, numbered chapter flags, finer ruler ticks, a playhead with a yellow head and a red line, and ◂ ▸ and eye in the track headers
- A new timeline's chapter 1 gets a poster frame, as in Encore (where something first shows when the start is black)
- A frame given in seconds sometimes showed the frame before it: fixed

### Menus

- A 16:9 DVD menu also carries the highlights for letterboxed viewing
- The Menu Viewer's bar is picture buttons in Encore's order. Normal / Selected / Activated go one at a time, as in Encore, and lay that state's subpicture over every button
- A menu with a video background sometimes stayed black in the Menu Viewer and in thumbnails: fixed
- The Library's kind toggles and actions are picture buttons as in Encore (a click shows that kind only, Ctrl+click adds or removes it)

### Flowchart

- Laid out and drawn as Encore's (link lines, tray, zoom, scroll bars, dragging, context menus). Readable with many links
- The pick whip links to chapter markers and menu buttons

### Screen

- All text in Encore's typefaces (Tahoma for Latin letters and digits, MS UI Gothic for Japanese; the Windows fonts are used)
- Fields, lists, group frames, panel menus (the ≡ at the right end of the tab strip) and the Build panel look like Encore's
- **Light appearance**: Preferences ▸ Interface ▸ Brightness switches between dark and light
- Errors, warnings and passes are told apart by a shaped mark as well as by colour, so that every kind of colour vision can tell them apart
- Progress is added to the upper right tabs
- Default names such as those of exports follow the display language

### Libraries

- FilmCraft, craft-libs, PhotoCraft, EffectCraft and SoundCraft updated to their latest versions

### Known limitations

- Importing from Blu-ray reads only discs without encryption. DTS and Dolby TrueHD sound and subtitles are not imported
- Writing to discs is Windows only. Only DVD-R has been tried in a real drive; DVD+R (dual layer included), DVD-RW, DVD+RW, BD-R and BD-RE not yet
- DV time code (subcode) is not read (Encore does not read it either). DVCPRO HD and DV inside MXF cannot be read
- Automatic routing still sometimes goes elsewhere than Encore's
- Not signed with a developer certificate. On Windows SmartScreen asks the first time the installer runs, on macOS Gatekeeper the first time (right-click ▸ Open, or remove it with `xattr -d com.apple.quarantine`)

## Earlier versions

The changes up to 0.1.0-alpha.12 are in the notes of each GitHub release ([v0.1.0-alpha.12](https://github.com/mOqOm/DiscCraft/releases/tag/v0.1.0-alpha.12) and the others).
