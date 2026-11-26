English | [日本語](CHANGELOG.ja.md)

# Changelog

What changed in each version of DiscCraft. The files for each system are on [GitHub's releases](https://github.com/mOqOm/DiscCraft/releases).

## 0.1.0-alpha.17 (2026-11-26)

### Menus

- Video thumbnail buttons in a menu made from a PSD are fixed. The button sets' artwork (the `(%)` video frame and the text) was drawn into one picture of the page, so moving or resizing a button left its frame and text behind, away from the video. As in Encore, a button set's artwork now moves and scales with its button (in motion and pop-up menus too)
- A PSD edited in PhotoCraft and taken back in kept the offsets the frame and the text had been given in DiscCraft before, so they were shifted again from their new place: fixed, they follow the PSD's layers

### Known limitations

- Importing from Blu-ray reads only discs without encryption. DTS and Dolby TrueHD sound are not imported; the disc's subtitles come into a Blu-ray project only, not yet into a DVD project
- Writing to discs is Windows only. DVD-R and DVD+R DL (dual layer) have been written in a real drive; DVD+R, DVD-RW, DVD+RW, BD-R and BD-RE not yet
- A disc label is printed from DesignCraft. DiscCraft does not yet place it where a printer's disc tray prints it: with a printer that prints the tray from an A4 page, place the label on that page in DesignCraft
- DV time code (subcode) is not read (Encore does not read it either). DVCPRO HD and DV inside MXF cannot be read
- Automatic routing still sometimes goes elsewhere than Encore's
- Not signed with a developer certificate. On Windows SmartScreen asks the first time the installer runs, on macOS Gatekeeper the first time (right-click ▸ Open, or remove it with `xattr -d com.apple.quarantine`)
- The menus in macOS's own menu bar have not been tried on a Mac yet
- A PSD button moved or resized in DiscCraft is still at its old place in the PSD when the menu is opened in PhotoCraft, and a PSD button's artwork does not turn when the button is rotated

## 0.1.0-alpha.16 (2026-11-26)

### Interface

- The Japanese interface now reads as the other ArtCraft Crafting Apps do: the words they share come from the Crafting Apps' shared translations (craft-i18n), and DiscCraft's own were translated again from the plain meaning of the English (エンドアクション → 終了時の動作, 再生リスト → プレイリスト, 初期設定ボタン → 既定のボタン, アクティベート時 → 決定時, ヒストリー → 履歴, "名前 :" → "名前:" and so on). The manual's names of the screen and its pictures follow
- The menu bar is the one the other Crafting Apps use (craft-menu). The menus and their items are as before. Alt or F10 opens them from the keyboard, and the Japanese menus get access keys in the form "ファイル(F)". On Windows About is last in Help and Preferences last in Edit. The display language is chosen in **Edit ▸ Preferences… ▸ Interface** (Window ▸ Language is gone; on macOS it is in the app menu). On macOS the menus are in the system's menu bar
- **Edit ▸ Undo / Redo** and **History** name the step in the English interface too ("Undo Add Button")
- **Help ▸ DiscCraft Help…** opens the chapter manual on GitHub. Resting the pointer on a Help item no longer shows the address it opens

### Command line

- The commands' names and groups, the parameter notes for agents, `disccraft-cli inspect` and the build's result are in English

### Known limitations

- Importing from Blu-ray reads only discs without encryption. DTS and Dolby TrueHD sound are not imported; the disc's subtitles come into a Blu-ray project only, not yet into a DVD project
- Writing to discs is Windows only. DVD-R and DVD+R DL (dual layer) have been written in a real drive; DVD+R, DVD-RW, DVD+RW, BD-R and BD-RE not yet
- A disc label is printed from DesignCraft. DiscCraft does not yet place it where a printer's disc tray prints it: with a printer that prints the tray from an A4 page, place the label on that page in DesignCraft
- DV time code (subcode) is not read (Encore does not read it either). DVCPRO HD and DV inside MXF cannot be read
- Automatic routing still sometimes goes elsewhere than Encore's
- Not signed with a developer certificate. On Windows SmartScreen asks the first time the installer runs, on macOS Gatekeeper the first time (right-click ▸ Open, or remove it with `xattr -d com.apple.quarantine`)
- The menus in macOS's own menu bar have not been tried on a Mac yet

## 0.1.0-alpha.15 (2026-11-26)

### Documents

- A user manual in chapters with pictures of the screen, in English and Japanese: `docs/manual/`, with a tutorial that makes a DVD with a menu from start to finish, a chapter on disc labels and a glossary with an index. Each chapter opens with what it lets you do, and the finer points sit in boxes of their own. The pictures are taken by DiscCraft itself, so they can be taken again for another language or a changed screen. The release also carries the manual as a PDF, one in English and one in Japanese (`disccraft-<version>-manual-en.pdf` / `-ja.pdf`)

### DVD

- The layer break of the last DVD-9 image built from a project is kept in the project and saved with it: the Build panel's Layer Break group shows it after the project is opened again, and writing that image to a dual-layer disc takes it even without the note beside the image

### Project Preview

- The Project Preview opens a disc whose menu buttons overlap, as Encore's does (the build still refuses it); before, it did not open and the status bar showed the check's errors as a raw dump
- The Project Preview looks more like Encore's: it starts at 100 % (the picture at its own size in the middle, not stretched to the window), the magnification sits under a screen icon, the box of what is playing shows three rows with icons (the menu or title, the button or chapter, still / motion or the time), Title and Menu are picture buttons, and the remote's arrows and Enter are one round pad

### Known limitations

- Importing from Blu-ray reads only discs without encryption. DTS and Dolby TrueHD sound are not imported; the disc's subtitles come into a Blu-ray project only, not yet into a DVD project
- Writing to discs is Windows only. DVD-R and DVD+R DL (dual layer) have been written in a real drive; DVD+R, DVD-RW, DVD+RW, BD-R and BD-RE not yet
- A disc label is printed from DesignCraft. DiscCraft does not yet place it where a printer's disc tray prints it: with a printer that prints the tray from an A4 page, place the label on that page in DesignCraft
- DV time code (subcode) is not read (Encore does not read it either). DVCPRO HD and DV inside MXF cannot be read
- Automatic routing still sometimes goes elsewhere than Encore's
- Not signed with a developer certificate. On Windows SmartScreen asks the first time the installer runs, on macOS Gatekeeper the first time (right-click ▸ Open, or remove it with `xattr -d com.apple.quarantine`)

## 0.1.0-alpha.14 (2026-11-25)

### Importing from Blu-ray

- In a Blu-ray project, the disc's subtitles (PG) come in too, as subtitle tracks in the disc's languages. They are taken out of the disc after the timeline is made, while you go on working (the status bar shows how far). Where a playlist cuts a clip while a subtitle shows, the subtitle is carried to the cut's start and cleared at its end. A DVD project does not take them yet

### DVD

- The disc usage of a timeline of several clips counted its clips' own bit rates as the rate that fits the disc, its converted Dolby Digital sound eight times over, and that for its first clip only: fixed (a 3-hour timeline of 7 clips was estimated at 8,198 MB for its rates, now 7,670 MiB)
- In a timeline of several clips (a Blu-ray's playlist imported, for one), a chapter point or a manual layer break in the second or a later clip starts exactly at its frame, as one in the first clip does (before, it moved to the nearest picture group, up to 7 frames away)
- The clips of a timeline of several clips were converted with a maximum video bit rate that left no room for the sound (it stayed at 9.8 Mbps): through a long busy passage, the video and the sound did not fit the DVD's mux rate, and the build stopped with "mux rate exceeded" while multiplexing, after the whole conversion. The maximum is now 9.8 Mbps less the sound's rate (192 kbps for Dolby Digital), as for a timeline of one video, so such timelines are converted again at the next build

### Writing to discs (Windows)

- The Build panel's **Source** can be a **Disc image** made before, as in Encore, when the output is the disc: **Build** writes that image as it is, without building the project again, and the image stays. A DVD-9 image is written with the layer break noted beside it; one larger than a layer without that note is not written to a dual-layer disc
- A DVD+R DL (dual layer) is written by DiscCraft's own commands to the drive. On the first real disc, Windows' writing set the disc's first layer to half the image on its own, not at the layer break DiscCraft had given, and gave up when the drive was slow to answer (at 16 %, leaving a disc that cannot be used). Now the first layer stays at the layer break, the writing waits for a busy drive and goes on from where the drive says, and each step is recorded in `<name>.burnlog.txt` beside the image
- Command line: `disccraft-cli disc-info` prints what the drive tells of the disc in a recorder (a DVD+R's media ID, whether it is empty, open or finalized, its tracks and where the next write goes, a dual-layer disc's first layer), for finding out what a writing that failed left behind
- A DVD+R DL a writing stopped on calls itself blank, but its first layer stays fixed: DiscCraft tells it apart and does not write it (part of it may be written), and the Build panel's disc line says so. `<name>.burnlog.txt` also records the drive's name, the disc's media ID, what the drive says while it is busy, and what it says of a write that failed
- A DVD+R DL written at a speed chosen was verified at that speed at most (the speed asked of the drive holds for reading too), and on the inner part of each layer at about half of it: a DVD-9 written at 2.4x read back at about 1x, so that its verify would have taken longer than the writing. The drive is given its own speeds back once the disc is closed (another program read the same disc at 4 to 7x)

### Disc labels

- **File ▸ Create Disc Label…** (DiscCraft's own) makes the label for a printable disc as a DesignCraft document, to print on the printer's disc tray: the disc's face 120 mm square, the printable ring (from 22 or 43 to 118 mm, as the disc's package says) as a path with a hole that a menu's background fills, so nothing is printed on the hole or past the edge, the disc's name and its named chapters as type, and guides that do not print (on a locked layer, so a logo or a track list pasted in later goes on the face and prints). Without DesignCraft, a PSD for PhotoCraft. Command line: `disccraft-cli disc-label`
- The disc label is linked to the project and kept when the project is saved, with its path relative to the project, so a project moved or copied together with it still finds it. **File ▸ Open Disc Label** opens it again in DesignCraft. The **Create Disc Label** dialog shows the project's label and opens it, links another file (or a label made beside the project before labels were linked), or unlinks it. Command line: `disccraft-cli disc-label … --link`

### Screen

- The disc usage counts what multiplexing adds to the video and sound (about 2 %; it was left out, so the bar showed more room than there was), shown orange in the bar and as its own part of the line under it, as the DVD-ROM content is green. A DVD9 with a manual layer break shows each layer (layer 0 up to the break, at most 4,076 MiB; layer 1 no more than layer 0) and the room left on each, and the project check warns when a layer would be too full
- While a build or a disc writing runs, the **Build Progress** dialog sits in the middle of the window, as in Encore, and the rest of the window cannot be used until it ends (the Build panel's settings, the menus and the keys could be used before, the recorder's **Refresh** among them, which asks the drive while it writes). It shows a bar for each thing the build is doing (the clips are transcoded side by side), and so does the Progress panel; they showed only the last one heard from. **Cancel** stops a build (what is transcoded stays for the next build). A writing shows its step, the time left at its pace, and **Stop** (stopped while the disc is verified, it says that the disc is written and closed but not verified; it said "Writing stopped")
- The window can be minimized while a build or a writing runs: on Windows its taskbar button shows how far it is (a moving bar while a build runs, the share written or verified while a disc is written, red when it failed), and lights up when it ends while the window is not in front
- A build that fails says why in a dialog (**Copy** puts the error on the clipboard). The Build panel used to go back to its settings without a word, the error only in the status bar and under Result at the panel's foot
- Sizes are shown in MiB and GiB, named as they are counted (1 MiB = 1,048,576 bytes): the usage bar, the disc's free space, the build's result and the Properties said "GB" or "MB" for them. A custom disc size is entered in GiB. The command line's build result and project summary say MiB and GiB too
- The buttons of the dialogs are drawn as in the other Craft apps' dialogs: round ends, the default answer (OK, Yes, Create, Close…) filled blue, the others an outline. The Welcome screen's buttons too
- Preferences is laid out as in the other Craft apps: a title bar, the categories in a framed list whose chosen row is shaded across, the labels in a column so that every field starts at the same place, the notes under the fields in small dim type, and the Media and Menus settings in titled groups

### Conversion speed

- Several clips converted side by side for a DVD no longer hold each other up: each picture's work (scaling, colour conversion, the encoder's slices) runs on one shared set of threads instead of new threads for every picture, and the programs reuse the memory of the pictures. Five clips at once convert 1.8 times as fast (they used about half the processor), one clip alone 1.3 times
- The clips of a timeline of several clips (a Blu-ray's playlist imported, for one) are converted for a DVD side by side, each on a processor core of its own, before they are joined; they were converted one after another on one core (a playlist of 7 clips took as long as all of them in a row)
- Two-pass conversion also runs on the GPU (the hardware encoder): one pass, then, only when the size came out more than 2 % off the target, once more with the average corrected, so the size lands within about 1 % of the target. Four to six times as fast as two passes on the processor, at a picture as good or better
- A 4K or 720p source made into a Blu-ray's 1080p is scaled as it is too, without the round trip through RGB (4K to 1080p about twice as fast)
- In a Blu-ray's HD conversion, a source already at the disc's size and colours (1080p BT.709, a Blu-ray's own video for example) goes to the encoder as decoded. Encoding on the GPU is nearly twice as fast (114 to 218 frames a second), and on the processor about a fifth faster. The round trip of the colours through RGB is gone, so the picture also stays as it was

### Known limitations

- Importing from Blu-ray reads only discs without encryption. DTS and Dolby TrueHD sound are not imported; the disc's subtitles come into a Blu-ray project only, not yet into a DVD project
- Writing to discs is Windows only. DVD-R and DVD+R DL (dual layer) have been written in a real drive; DVD+R, DVD-RW, DVD+RW, BD-R and BD-RE not yet
- A disc label is printed from DesignCraft. DiscCraft does not yet place it where a printer's disc tray prints it: with a printer that prints the tray from an A4 page, place the label on that page in DesignCraft
- DV time code (subcode) is not read (Encore does not read it either). DVCPRO HD and DV inside MXF cannot be read
- Automatic routing still sometimes goes elsewhere than Encore's
- Not signed with a developer certificate. On Windows SmartScreen asks the first time the installer runs, on macOS Gatekeeper the first time (right-click ▸ Open, or remove it with `xattr -d com.apple.quarantine`)

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
