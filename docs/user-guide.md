# DiscCraft User Manual

English | [日本語](user-guide.ja.md)

How to use DiscCraft, in the order the work goes. Names of commands and
panels are the ones the English interface shows. Chapters 1 to 13 are the reference for every screen;
[chapter 14](#14-advanced-techniques) is the recipes: scene-selection menus, hidden buttons, routing by
hand, motion menus, bit budgeting.

## Contents

1. [About DiscCraft](#1-about-disccraft)
2. [Getting started](#2-getting-started)
3. [The screen](#3-the-screen)
4. [Importing media](#4-importing-media)
5. [Making a timeline](#5-making-a-timeline)
6. [Making a slideshow](#6-making-a-slideshow)
7. [Playlists and chapter playlists](#7-playlists-and-chapter-playlists)
8. [Making menus](#8-making-menus)
9. [Building the navigation](#9-building-the-navigation)
10. [Subtitles](#10-subtitles)
11. [Checking the project](#11-checking-the-project)
12. [Previewing](#12-previewing)
13. [Building](#13-building)
14. [Advanced techniques](#14-advanced-techniques)
15. [Preferences](#15-preferences)
16. [Keyboard shortcuts](#16-keyboard-shortcuts)
17. [Command line and scripts](#17-command-line-and-scripts)
18. [Using it from an AI agent (MCP)](#18-using-it-from-an-ai-agent-mcp)
19. [Limits and rules](#19-limits-and-rules)
20. [When something does not work](#20-when-something-does-not-work)

## 1. About DiscCraft

DiscCraft is authoring software that makes DVD-Video and Blu-ray (BDMV) discs from video, audio and still images.
You put together menus, chapters, several audio and subtitle tracks and the navigation; media that does not fit
the format is converted automatically; the result is written to a folder or an ISO image.

What it does:

- DVD-Video (`VIDEO_TS` folder, ISO) and Blu-ray (`BDMV` folder, ISO)
- Still menus and motion menus, buttons and highlights, menus made from PSD files
- One title made of several video clips, chapters, audio and subtitle tracks
- MPEG-2 video that already fits a DVD, and AC-3 / WAV / DTS audio, go on the disc as they are, without conversion
- Slideshows, playlists, chapter playlists, First Play, the Title button
- A preview without building (Project Preview), and playback of the built disc in the Monitor
- The same operations from the GUI, the command line and MCP (every operation is an engine command)

What it does not do (for now):

- Burning discs. It makes folders and ISO images; burning is left to other software
- Copy protection (CSS, AACS and the like). Only the region code flags
- BD-J menus (Blu-ray is HDMV only), encoding DTS / Dolby TrueHD / DTS-HD (a DTS file can go on a DVD as it is)
- Editing video itself (edit in FilmCraft, make the disc in DiscCraft)
- 4K (Ultra HD Blu-ray)

Blu-ray video is SD (720×480 / 720×576) MPEG-2 or HD. HD is H.264 for 1080p (23.976 / 24) and 720p, and
MPEG-2 for 1080i (59.94 / 50). Sources shot at 29.97 or 25 fps become HD at their own speed with 1080i 59.94 / 50
(the Blu-ray specification has no 1920×1080 at 29.97p / 25p, so each frame goes on the disc as the two fields of 1080i).
HD video averages 12 Mbps as H.264 and 25 Mbps as 1080i MPEG-2. When the titles are too long to fit the disc at that rate,
it is lowered automatically until they fit (down to 3 Mbps for H.264 and 8 Mbps for MPEG-2; past that the disc cannot be built).
The capacity meter and Check Project estimate with the same bitrate.
H.264 in an `.m2ts` / `.mkv` / `.mp4` / `.mov` that a Blu-ray can carry goes on the disc as it is, without conversion (see 4.1).

## 2. Getting started

### 2.1 The Welcome screen

The Welcome screen opens at startup.

- **Recent Projects**: files you have opened (up to 10). Click a row to open it.
  **Remove from List** in the right-click menu takes it off the list (the file stays). A file that is gone is grey
  and cannot be opened
- **New Project**: opens the New Project dialog
- **Open Project…**: choose a `.dcproj`
- **Exit**: quits the application

After the Welcome screen is closed, **File ▸ New ▸ Project** (Ctrl+N) and
**File ▸ Open Project…** (Ctrl+O) do the same.

### 2.2 New project

The **New Project** dialog has two tabs, **Basic** and **Advanced**.

| Tab | Fields |
|---|---|
| Basic | Name, location, authoring mode (DVD / Blu-ray), television standard (NTSC / PAL), a summary of the transcode settings |
| Advanced | Aspect ratio (4:3 / 16:9), disc size (DVD5 / DVD9, BD25 / BD50 / BD100 / BD128) |

- With an empty location, you are asked for one when you save
- The default television standard can be changed in [Preferences](#15-preferences)
- **OK** creates `<location>/<name>.dcproj` at once and editing starts

What you can change later: authoring mode, aspect ratio and size in **File ▸ Project Settings…**; the disc's name
and size under **Disc Info** in the **Build** panel; all the disc's settings in the **Disc** panel
(the Blu-ray video size, SD (MPEG-2) / HD (H.264), is there too).

### 2.3 Saving

- **File ▸ Save** (Ctrl+S). Building saves first as well
- **File ▸ Save As…** (Shift+Ctrl+S)
- Closing a project, opening another or quitting with unsaved changes asks first

A project (`.dcproj`) is a JSON text file. Media is not copied; it is referred to by path.
Media that is moved elsewhere becomes "offline"; tell DiscCraft the new place with **Replace Asset…**.
A `<project>.menus` folder may appear beside the project (it holds the PSD files of menus edited in PhotoCraft
or another application).

### 2.4 Where settings are kept

The workspace layout, the display language, the recent projects and the column widths are kept in a settings file.

| Place | Contents |
|---|---|
| Windows: `%APPDATA%\disccraft\workspace.json`<br>Others: `~/.config/disccraft/workspace.json` | Workspaces, panel layout, display language, recent projects, default television standard |
| `library.json` in the same folder | Your own menus and buttons added to the Library |

## 3. The screen

The toolbar is under the menu bar, the status bar is at the bottom, and panels (docking tab groups) fill the rest.
The status bar shows the result of the last operation and the progress of a build (a part of DiscCraft that Encore
does not have).

### 3.1 Menu bar

Eight menus: **File / Edit / Object / Menu / Timeline / View / Window / Help**.
Items and shortcuts follow Encore's defaults. The ones used most:

- **File ▸ New ▸** (Project / Menu / Timeline / Slideshow / Playlist / Chapter Playlist)
- **File ▸ Import ▸** (Asset… / Menu… / Pop-up Menu… / Menu Page… / Timeline… / Slideshow…)
- **File ▸ Preview / Check Project… / Build ▸ Folder… · Image…**
- **Edit ▸ Preferences…**, **Edit ▸ History ▸** (step back through the list of operations that can be undone)
- **Object ▸** (Convert to Button / Convert to Object, Specify Link…, Drop Shadow…, Flip, Rotate, Arrange, Align, Distribute)
- **Menu ▸** (New Menu, Create Chapter Index, Replace Menu…, Add New Page, Edit Menu Color Set…,
  Set Menu User Operations…, Save Menu as File…, Save Menu as Template…, Set End Action…)
- **Timeline ▸** (New Timeline / New Slideshow / New Chapter Playlist, adding and removing tracks,
  importing and exporting subtitles, adding chapter points)
- **View ▸** (zoom, the three subpicture states, safe area, button routing, guides, chapter names)
- **Window ▸** (workspaces, showing and hiding panels, display language)
- **Help ▸** (Join the ArtCraft Discord… / ArtCraft Website / DiscCraft on GitHub / Report an Issue… /
  Reveal Log Files… / System Compatibility Report… / Check for Updates… / About DiscCraft / DiscCraft Help… (F1)).
  Resting the pointer on an item shows the address it opens or what it does. **DiscCraft Help…** opens this manual

### 3.2 Toolbar

Menu editing tools on the left, the workspace choice at the right end.

| Tool | Key |
|---|---|
| Selection tool | V |
| Direct Select tool | A |
| Move tool | M |
| Rotate tool | R |
| Text tool | T |
| Vertical Text tool | Y |
| Zoom tool | Z |

To their right are the **Edit Menu in PhotoCraft** (Shift+Ctrl+M) and **Preview** (Alt+Ctrl+Space) buttons.

### 3.3 Panels

The default workspace is laid out as in Encore.

| Place | Tabs |
|---|---|
| Left | Project / Menus / Timelines / Build |
| Centre | Monitor / Menu Viewer / Flowchart |
| Upper right | Properties / Disc |
| Lower right | Library / Layers |
| Bottom | Timeline Viewer / Slideshow Viewer |

- **Project**: the list of every item (assets, timelines, menus, slideshows, playlists, chapter playlists,
  folders). Columns can be sorted, shown or hidden and resized. The right-click menu has Open / Rename… /
  Set as First Play / Replace Asset… / Show in Explorer / Delete and more
- **Menus**: the list of menus and the buttons of the selected menu
- **Timelines**: the list of timelines
- **Build**: the [Build panel](#13-building)
- **Monitor**: playback of the built disc, and the preview of a timeline. The control bar below has the timecode
  and zoom, a scrub bar with chapter flags, play / stop / previous and next chapter / one frame back and forward /
  Add Chapter Point / Poster Frame, the safe area and subtitle buttons, and **Open Build Output**
- **Menu Viewer**: the menu's canvas. The tab is named after the open menu
- **Flowchart**: a diagram of the items and their links. Nodes can be dragged
- **Properties**: the sheet of the selected item. With nothing selected, the disc's settings
- **Disc**: the disc's name, description, aspect ratio, audio / subtitle, user operations
- **Layers**: the menu's layers (eye and lock, `(+)` button sets, `(=1)`–`(=3)` highlights, T for text, % for shapes;
  the background is fixed)
- **Library**: DiscCraft's own presets (menus / buttons / images / backgrounds / shapes) and your own items
- **Timeline Viewer**: a timeline in the same form as FilmCraft's (V1 / A1 / S1 headers, waveform, chapter flags)
- **Slideshow Viewer**: the list of slides and their order, scaling and transitions

### 3.4 Rearranging panels and workspaces

Tabs can be dragged to reorder them, move them to another group, split a group or float them. A middle click or ×
closes one; opening it again from the **Window** menu puts it back where it was. The layout is saved automatically
for each workspace.

**Window ▸ Workspace** lists four built-in workspaces and the ones you saved.

| Name | What comes to the front |
|---|---|
| Default | Encore's default layout (all groups) |
| Menu | The list of menus, the Menu Viewer, Layers |
| Navigation | The Flowchart |
| Build | The Build panel and Disc |

**New Workspace…** saves the current layout under a name (up to 32, names up to 64 characters).
**Delete Workspace** and **Reset “name”** are here too. The **Workspace** combo at the right end of the toolbar
switches as well.

## 4. Importing media

### 4.1 How to import

- **File ▸ Import ▸ Asset…** (Ctrl+I)
- Drag and drop files onto the window (a dropped folder gives the media files inside it; up to 500 at a time)
- **File ▸ Import ▸ Timeline…** (Shift+Ctrl+T): makes an asset and a timeline for each chosen video
- **File ▸ Import ▸ Slideshow…** (Shift+Ctrl+G): makes one slideshow with the chosen images as its slides
- **File ▸ Import ▸ Menu… / Pop-up Menu… / Menu Page…** (Shift+Ctrl+I / Shift+Ctrl+K /
  Shift+Ctrl+U): makes a menu from a layered PSD (or a PhotoCraft `.pcraft`)

A large video hours long (an mkv or mp4 of tens of gigabytes) is opened in the background right after it is imported.
The window does not stop meanwhile; the status bar says "Reading media…", and when the file is open its length, size and
thumbnail and the disc's usage come in. A file's length, size and thumbnail are remembered once it has been opened, so
the next time the project is opened there is no wait (while the file's place, size and modification time stay the same).
The length, size and frame rate of an mkv (H.264) are read from the front of the file, so they and the disc's usage are
there at once; only its thumbnail waits for the file to be opened.
The frames shown in the Monitor and in menus, and the pictures of a timeline's clips, also stay empty until the file is
open, and the window does not stop.
The Monitor's picture is decoded in the background, so while the playhead is dragged it shows each frame that is ready
in time.

Interlaced H.264 (1080i from a camcorder or a recorder) cannot be decoded to pictures yet. It imports, and its length and
size are shown, but the thumbnail and the Monitor stay empty and a build cannot convert it. A Blu-ray can still carry it
as it is, without conversion ("H.264 that goes on an HD Blu-ray as it is" below). For a DVD, convert it to progressive first.
A video whose picture cannot be decoded still plays its sound on the timeline. The Monitor shows a black screen with the
reason, and the time and the playhead move on, so chapter points can be placed by ear.

**H.264 that goes on an HD Blu-ray as it is**: H.264 in an `.m2ts` / `.ts` / `.mkv` / `.mp4` / `.m4v` / `.mov` goes on the disc without re-encoding
(the picture quality is kept, and the build is fast) when all of these hold.

- Its size, frame rate and scan are those of Blu-ray Video in the disc's properties (interlaced 1920×1080 at
  29.97 / 25 fps for 1080i 59.94 / 50; progressive for 1080p 23.976 / 24 and 720p)
- High or Main profile, up to level 4.1, 4:2:0 at 8 bits, and no more than 40 Mbps averaged over the whole file
- It starts with a key frame (IDR)
- It is not trimmed on the timeline (for 1080i, it is the timeline's only clip)

Such a file shows "Don't Transcode" as its transcode status in the Project panel, and the size estimate counts the file's size.
The sound is handled as before (anything but AC-3 and WAV is converted to Dolby Digital). Other files are converted.

### 4.2 Files that can be read

| Kind | Extensions |
|---|---|
| Video | mp4, mov, m4v, avi, mkv, webm, mxf, m2v, mpg, mpeg, vob, m2ts, mts, m2t, ts |
| Audio | wav, ac3, dts, mp3, aac, m4a, mka, mp2, ogg, oga, opus, flac, aif, aiff |
| Images | png, jpg, jpeg, bmp, tif, tiff, psd, psb, pcraft, svg |
| Subtitles | srt, vtt, txt, idx (VobSub), sup (PGS) |
| Menu designs | psd, pcraft |
| Craft documents | `.scraft` (a SoundCraft session → audio), `.ecproj` (an EffectCraft project → video), `.fcproj` (a FilmCraft sequence → a timeline) |

A WAV goes on the disc as it is (LPCM) when it is 16-bit 48 kHz; any other (44.1 kHz, 24-bit and so on) is converted.
When a video file holds several audio streams, only the first is used.

SVG is drawn at the size it is shown, so it does not blur. Subtitle text is read as UTF-8 / UTF-16 / Shift_JIS.

When an EffectCraft project (`.ecproj`) has several compositions, choose the one to use under **Composition** in
the asset's properties (without a choice, the composition that was open in EffectCraft). Import the same file once
more and each asset can use a different composition.

A DTS file (`.dts`) goes into a DVD's audio track as it is, without decoding (48 kHz, up to 1509.75 kbps).
It cannot go on a Blu-ray yet, and it is silent in the Monitor and the preview.

### 4.3 About media

- The row in the Project panel shows the size, frame rate, length and whether a conversion is needed
- Media that does not fit the format is converted automatically at build time (audio to Dolby Digital (AC-3)
  192 kbps; the video bit rate is chosen to fit the disc, up to 9.8 Mbps by default)
- MPEG-2 video that already fits a DVD is not converted: elementary streams (`.m2v`), program streams with
  video and sound together (`.mpg` / `.vob`; their AC-3 sound as it is), and 704×480, 352×480 and 352×240 video
  (576 / 288 lines for PAL) are all used as they are. An HD Blu-ray runs no conversion for DVD
- **Edit ▸ Edit Original** (Ctrl+E) opens the media in the application that made it (PhotoCraft for images,
  SoundCraft for audio, FilmCraft or EffectCraft for video). It is read again automatically when you save
- The Project panel's right-click menu has **Replace Asset…** (tell it where offline media is now) and
  **Show in Explorer**
- **Transcode Now** in the right-click menu converts that media ahead of the build. It runs in the background (the
  status bar shows how far it is), and you can go on editing meanwhile. When it is done the transcode status becomes
  "Transcoded", and the build uses that conversion. The conversion is kept inside the Build panel's destination. It
  cannot be chosen for media that needs no conversion, nor for media whose picture cannot be decoded (what cannot
  be decoded cannot be converted either). **Revert to Original** throws a finished conversion away, so the status is
  "Untranscoded" again (the next build, or Transcode Now, makes it anew)
- Media in use cannot be deleted. Taking an item out of use sets the links to it back to Not Set
- **Copy** and **Paste** in the Project panel's right-click menu make a copy of a menu (with its pages), a timeline, a
  slideshow, a playlist or a chapter playlist. The copy is named with a number ("Name 2") and goes in the same folder.
  Its links to other items are kept, and a link it had to itself points at the copy

## 5. Making a timeline

### 5.1 Creating

Select a video asset and choose **File ▸ New ▸ Timeline** (Ctrl+T), or **Timeline ▸ New Timeline**.
With no video selected, choose one from the list of videos in the **New Timeline** submenu.
A DiscCraft timeline always has a video as its first clip.

For a work edited in FilmCraft there is **Timeline ▸ New Timeline from FilmCraft Sequence…**.
**Timeline ▸ Import FilmCraft Markers…** adds the sequence's markers to the open timeline as chapter points.

### 5.2 Several video clips

Drag another video onto the Timeline Viewer and it is butted on right after the clip before it, with a chapter at
the join (up to 99 clips in one title). Drag a clip's edge to trim it; the cut snaps to the start of an MPEG-2 GOP.

- MPEG-2 clips that need no conversion pass through as trimmed
- Clips that need a conversion (H.264 and the like) can be joined and built too, but on a DVD they are not cut at
  the trimmed place
- An HD Blu-ray encodes each clip to H.264 and joins them at IDR pictures, so trims come out as set
- The first clip can be removed too (the second becomes the new first)
- Drag a clip's body left or right to move it. The gap this leaves in front of it plays black on the disc
  (playback in the Monitor skips gaps). Clips cannot be put over their neighbours
- Program stream clips (`.mpg` / `.vob`) snap chapters and trims to GOP starts as well

Audio and subtitle media dropped on the Timeline Viewer go into A1 / S1.

### 5.3 Chapter points

- **Timeline ▸ Add Chapter Point** (`*` on the numeric keypad) at the playhead
- The flags under the ruler of the Timeline Viewer can be dragged
- **Add Chapter Points at Interval…** (5 minutes by default) sets many at once
- Select a flag and **Edit ▸ Clear** (Delete) removes that chapter
- Name a chapter in its properties (an empty name shows as "Chapter N"). **View ▸ Show Chapter Names** (Ctrl+8)
  shows the names on the timeline
- The **Poster Frame** button of the Monitor sets the poster frame to the picture at the playhead

A DVD takes up to 99 chapters in a title. For a menu of scene buttons that show these chapters, see
[scene-selection menus with pictures](#141-a-scene-selection-menu-with-pictures).

### 5.4 Tracks

**Timeline ▸ Add Audio Track… / Add Subtitle Track…** add one track at the bottom;
**Remove Audio Track / Remove Subtitle Track** remove the bottom one. The **Disc** panel (the same as the properties
with nothing selected) sets the audio track heard and the subtitle shown by default. In the Monitor, the speaker
button in a track's header chooses the track you hear.

An audio track's clip can be dragged left and right to move where it starts (by frames). The track is silent up to
there. A Dolby Digital (AC-3) file only gets silence of its own form in front and is not re-encoded; other sound gets
the silence and is converted to Dolby Digital. A DTS track cannot be moved (the build reports an error; move it back to
the start).

The limits are 8 audio tracks and 32 subtitle tracks.

### 5.5 End action, override, remote menu

The timeline's sheet in the Properties panel has the same three link fields as Encore.

| Field | Meaning |
|---|---|
| End Action | Where to go after it has played to the end (a menu, another timeline, a slideshow, a playlist, nothing) |
| Override | Where to go after the end action's target has finished (a flow like warning → feature → extras menu) |
| Remote Menu | The menu the remote control's Menu button shows (the pop-up menu on a Blu-ray) |

Links can be set in five ways ([five ways to link a button](#147-five-ways-to-link-a-button)). The three quickest:

- Press **▶** to the right of the field, then click an item in the Project panel (the pick whip)
- **Object ▸ Specify Link…** (Ctrl+L) and choose in the dialog
- Drag from node to node in the Flowchart

## 6. Making a slideshow

**File ▸ New ▸ Slideshow** (Ctrl+G) makes an empty slideshow; drag images onto the Slideshow Viewer.
Selecting several images and choosing **File ▸ Import ▸ Slideshow…** (Shift+Ctrl+G) makes one too (named after
the first image, with the default slide length of 6 seconds).

| Setting | Contents |
|---|---|
| Slide duration | 6 seconds by default. Entered as a timecode (00;00;06;00 for NTSC) |
| Scale | Scale and Crop to Fit / fill / no scaling |
| Transition | None / Cross Dissolve / Dip to Black / Wipe. 0.1 to 10 seconds long (2 seconds by default). Each slide can have its own |
| Manual advance | When on, each slide waits until the viewer moves on with the remote control |
| Pan & Zoom | With **Pan & Zoom** on in a slide's properties, the picture moves slowly while the slide is up (Pan: None / Pan Left / Pan Right / Pan Up / Pan Down; Zoom: Zoom In / Zoom Out / None). The slideshow's **Random Pan & Zoom** gives a motion to every slide that has no setting of its own |
| Create Subtitle | Makes subtitles from each slide's name and description (**Include** also takes the next slide's name). Each slide can choose its highlight (group), stroke and alignment; the colours come from the slideshow's colour set |

Drag slides to reorder them; **Edit ▸ Clear** (Delete) removes them. Adding audio lets you fit the length to it.
Write a slide's name and description in its properties (as subtitles they also become the disc's chapter names).

## 7. Playlists and chapter playlists

### 7.1 Playlists

**File ▸ New ▸ Playlist…** makes one with a name; drag items (timelines, slideshows) into its properties.
Items can be moved up and down, removed, and opened with a double click. A menu button can link to a playlist.

### 7.2 Chapter playlists

Select a timeline and choose **Timeline ▸ New Chapter Playlist**. It makes a list of the timeline's chapters in the
order they are to play (so extras can be watched in any order).

- Choose **Sequential / Random / Shuffle** (random and shuffle work on Blu-ray as well)
- An entry's target can be changed, and chapters of other timelines can be added
- The Chapter Playlist Viewer previews it

### 7.3 Chapter index

**Menu ▸ Create Chapter Index** makes a menu with a button for each chapter.

- When the menu open in the viewer has chapter buttons, the remaining chapters are assigned after the buttons
  already linked (in this case at least one chapter button must be linked to a timeline)
- Otherwise it makes a menu with a button for each chapter of the selected timeline (DiscCraft's own way)
- **Create Multi-Page Chapter Index** splits them 6 buttons to a page and adds **Previous / Next** buttons
- A page holds up to 18 chapter buttons

## 8. Making menus

### 8.1 Ways to make one

| Way | Result |
|---|---|
| **File ▸ New ▸ Menu** (Ctrl+M) | An empty menu, with the same guides as Encore |
| **File ▸ Import ▸ Menu…** (Shift+Ctrl+I) | Makes a menu from a PSD / `.pcraft`. Groups whose names start with `(+)` become buttons |
| **New Menu** on a **Library** preset | Made from one of DiscCraft's own templates |
| **Menu ▸ Add New Page** | The next page of the open menu (same background and settings) |

The first menu made becomes **First Play** and the **Title Button** automatically.

The rules for PSD layer names: a group starting with `(+)` is a button; `(=1)` / `(=2)` / `(=3)` are the pictures
of the normal / selected / activated highlights; `(%)` is a video thumbnail button; `(^)` is auto activate.

### 8.2 Buttons

- **Menu ▸ Add Button**, or select existing text / an image / a shape and **Object ▸ Convert to Button** (Ctrl+B)
- Drag to move, use the handles to resize, Shift to select several
- **Object ▸ Align / Distribute / Arrange** (Bring Forward Ctrl+], Bring to Front Shift+Ctrl+], Send Backward
  Ctrl+[, Send to Back Shift+Ctrl+[). With **Align Relative to Safe Area** on, the safe area frame is the reference
  instead of the canvas
- **Cut / Copy / Paste / Duplicate** (Ctrl+X / Ctrl+C / Ctrl+V / Ctrl+D), **Clear** (Delete)
- Up to 36 buttons in a menu. The **Default Button** (the one the remote control's arrows highlight first) is set
  in the properties or by right click
- Buttons may overlap while you edit, but **their highlights do not overlap on the disc** (overlapping buttons get a
  red frame and the check reports "Buttons overlap.")
- **Object ▸ Convert to Object** turns a button back into text

Button properties:

| Field | Contents |
|---|---|
| Link | Where to go when it is pressed |
| Override | Where to go after the link's target has finished |
| Type | Normal / Chapter / Next / Previous |
| Auto Activate | The button under the cursor is activated without Enter |
| Audio / Subtitle | The audio and subtitle this button switches to |
| Routing | Where the arrow keys move (automatic / manual) |
| Transition | The motion that goes with the button in a motion menu |

**View ▸ Show Button Routing** (Ctrl+7) shows the cross of routes. The routing defaults (movement up, down, left and
right, how to wrap, whether to leave out "Next" and "Previous" buttons) and the syncing of button names are set in
[Preferences ▸ Menus](#15-preferences) (kept in the project). To lay the routes out yourself, turn off
**Automatically Route Buttons** and drag the arrows: [routing and numbering by hand](#143-routing-and-numbering-by-hand).

### 8.3 Text and shapes

- Click with the **Text tool** (T) or the **Vertical Text tool** (Y) and type. Font, font style, size, leading,
  tracking, baseline, superscript / subscript, colour and anti-aliasing can be set
- Put **Images** and **Shapes** (rectangle / rounded rectangle / ellipse) from the **Library** on the menu with
  **Place**, or by dragging. The **Layers** panel handles their order and the eye / lock (the background cannot be
  moved)
- **Object ▸ Drop Shadow…** (Shift+Ctrl+O), **Flip Horizontal / Flip Vertical** (Alt+Ctrl+H /
  Alt+Ctrl+V), **Rotate ▸** (180°, 90° clockwise / counter-clockwise). Any angle with the Rotate tool (R)
- **Guides**: **View ▸ New Guide…** (Shift+Alt+Ctrl+:) sets one at a position you type; **Show Guides** (Ctrl+:),
  **Snap to Guides** (Shift+Ctrl+:), **Lock Guides** (Alt+Ctrl+:) and **Clear Guides** handle them

### 8.4 Colours and highlights

**Menu ▸ Edit Menu Color Set…** sets the normal / selected / activated colours
(highlight frames, text and subpictures). **Highlight groups** let several buttons light up together.
**View ▸ Show Normal Subpicture / Show Selected Subpicture / Show Activated Subpicture** (Ctrl+3 / Ctrl+4 / Ctrl+5) compare them on the canvas.
For choosing the groups button by button, and for invisible highlights, see
[highlight colours, button by button](#145-highlight-colours-button-by-button).

### 8.5 Background and motion menus

Set in the menu's sheet of the Properties panel (**Basic / Motion / Pop-up**).

- The background is a colour / an image / a video asset (not a timeline). With video it becomes a motion menu
- Loop count and loop point, duration (a menu is at most 254 seconds)
- **End Action** (where to go when time runs out; Menu ▸ Set End Action… opens it too) and **Override**
- The **Pop-up** sheet makes it a pop-up menu and sets its background timeline / background frame (Blu-ray)

**Menu ▸ Create EffectCraft Composition…** makes an EffectCraft project from the menu's layers; the rendered movie
can be the menu's background (a motion menu). [Motion menus, with sound and movement](#146-motion-menus-with-sound-and-movement)
is the whole job, including the loop, the audio and what to hide afterwards.

### 8.6 Reworking in a PSD

- **Edit ▸ Edit Menu in PhotoCraft** (Shift+Ctrl+M): turns the menu into a layered PSD and opens it in PhotoCraft
  (or the default application). It is read again automatically when you save. The PSD is kept in
  `<project>.menus/`. Afterwards button links are as before, new groups add buttons and the buttons of groups that
  are gone are removed
- **Menu ▸ Save Menu as File…**: writes the menu as a PSD
- **Menu ▸ Replace Menu…**: replaces the open menu with a PSD's design, keeping the links by button number
- **Menu ▸ Save Menu as Template…** (Ctrl+Alt+M): saves it in the Library's **User** set

### 8.7 User operations

**Menu ▸ Set Menu User Operations…** and **File ▸ Set Disc User Operations…** decide what the remote control may
and may not do during playback (25 user operations on a DVD, the UO mask on a Blu-ray).

## 9. Building the navigation

### 9.1 First Play and the Title button

**First Play** is what runs first when the disc is put in; the **Title Button** is where the Title button goes.
They are set automatically when the first timeline or menu is made. To change First Play, right-click an item in
the Project panel or the Flowchart and choose **Set as First Play** (the Title Button is set in the properties).

### 9.2 Flowchart

**Window ▸ Flowchart** (F5). Items are boxes and links are arrows.

- Boxes can be dragged. **Auto Layout** / **Auto Layout All** tidy them up
- Drag from a button or an end action to a menu, timeline, slideshow or playlist to link them
- The right-click menu has Open / Specify Link / Rename / Set as First Play and more
- A row of **unlinked items** appears below (useful for finding orphans)
- **View ▸ Show All Thumbnails** puts pictures in the boxes

For reading a whole disc off the page — First Play, menus that lead back up, what nothing links to — see
[the Flowchart as a map of the disc](#148-the-flowchart-as-a-map-of-the-disc).

### 9.3 Kinds of links

| Place | What it decides |
|---|---|
| Button | Link (where pressing goes), override (where to go after the target), routing (arrow key movement) |
| Timeline | End action, override, remote menu |
| Menu | End action (when time runs out), override, duration |
| Disc | Override (the disc's default destination) |

## 10. Subtitles

### 10.1 Importing

- **Timeline ▸ Import Subtitles…** (SRT / WebVTT / text with timecodes; UTF-8 / UTF-16 / Shift_JIS)
  adds one subtitle track to the selected timeline
- Or import a subtitle file as an asset and drop it on the Timeline Viewer to make the clips
- Text scripts written by Encore (UTF-16 LE) can be read too. **Timeline ▸ Export Subtitles ▸
  Subtitle Track** writes SRT / text scripts
- **Create Subtitle** of a slideshow makes subtitles from the slides' names and descriptions

### 10.2 Editing

On the S1 track of the Timeline Viewer, drag a clip's edge to trim it and double-click to change the text
(Enter / Shift+Enter for a new line, Esc to finish). The shortest trim is 0.4 seconds (13 frames NTSC, 11 PAL).

The subtitle buttons of the Monitor work too (**Previous Subtitle / Next Subtitle / Trim Subtitle In-Point to
Here / Trim Subtitle Out-Point to Here / Add Subtitle**). **Add Subtitle** puts a clip of 2 seconds at the playhead
(it needs 2 free seconds before the next subtitle).

### 10.3 Appearance

Select a subtitle clip and the Properties panel shows the **Subtitle Clip** sheet.

| Field | Contents |
|---|---|
| Text / Duration / In point / Out point | The text and the times |
| Highlight | Group 1 to 3 (a set of colours). **Track Setting** follows the track's group |
| Stroke | None / Thin / Medium / Thick / Extra Thick |
| Align | The vertical place (100 % is the bottom) |
| Justify | Left / Center / Right (centre by default; left and right stand on the title-safe edges) |

The colours are set under **Color Set** in the properties of the timeline (for slide subtitles, of the slideshow).

- Each of groups 1 to 3 has a colour and an opacity for the fill, the stroke and the anti-alias. The default
  colours are Encore's
- With **Create anti-alias color from fill and stroke** on, the anti-alias colour (the one that softens the edge of
  the letters) is halfway between the fill and the stroke. Turn it off to choose your own
- Type a **Name** and press **New** to save the current colours as a colour set of the project. Saved sets can be
  chosen in the **Color Set** list and removed with **Delete** (a timeline holds a copy of the colours, so changing
  a set later does not change it)
- **Export… / Import…** exchange a colour set as a file (`.dccolors`). **Reset to Default** brings back the original
  colours

Keep subtitle text inside the title-safe area (80 % of the frame). **View ▸ Show Safe Areas** (Ctrl+6) shows it.
Subtitles that do not fit are reported by the [project check](#11-checking-the-project).

## 11. Checking the project

**File ▸ Check Project…** (Shift+Ctrl+L), the **Check Project** button on the right of the **Build** panel,
or F4. In the check window, tick the items under **Check** and press **Start**; the results come out by item in two
columns, **Name** and **Problem** (an item with several problems is folded into "{n} errors"; the wording follows
Encore).

| Category | What it looks at |
|---|---|
| Button Links | Buttons whose target is missing or whose link is not set, buttons outside the frame, a default button that does not exist |
| First play | Whether First Play is set |
| Orphans | Timelines and menus no item leads to |
| End actions | Items whose end action is not set |
| Title Button | Whether the Title Button is a menu |
| Playlists | Empty playlists and slideshows, chapter playlists without entries, slides with a bad duration |
| Overrides | The override settings |
| Remote Menus | Whether the remote menu is a menu |
| Overlapping Buttons | Buttons whose highlights overlap, menus with too many buttons |
| Menu Total Size | Menus without buttons, durations, a mix of 4:3 and 16:9 |
| Disc capacity | Whether the disc is over its size, whether the bit rate is too low |
| Chapters and Trims | More than 99 chapters, the first chapter not at the start, chapters out of order |
| Timeline Bitrate Too High | On a DVD, timelines whose video (used as it is) and audio tracks together are over 9.8 Mbps |
| Subtitle Text Overflow | Subtitles whose text does not fit the title-safe frame (11 lines or more at the standard NTSC size, or a long line that cannot be broken). The subtitle track and the in / out times are shown |
| Subtitle Spacing | On a Blu-ray (HD or PAL), subtitles less than 5 frames after the one before on the same track. Not looked at for a DVD |
| Track and title limits | 8 audio tracks, 32 subtitle tracks, 99 DVD titles and so on |
| Media files | Media that is offline, missing or of a kind that cannot be used |
| Other | Anything that fits none of the above |

The **Build** button always runs the check before building; when there are problems a dialog offers
**Show** (open the check window) / **Ignore and Continue** / **Cancel**.

## 12. Previewing

### 12.1 Project Preview (preview without building)

**File ▸ Preview** (Alt+Ctrl+Space), or the play button of the toolbar. Without building, a window opens that plays
the project as it is with DiscCraft's own VM.

- The control bar below has the zoom, the audio and subtitle tracks, the current item (name, button or chapter
  number, still / motion or the playing time), **Title** / **Menu** (**Pop-up Menu** on a Blu-ray), the arrow keys
  and Enter, previous chapter / stop / pause / play / next chapter, **Run the End Action**, **Go to Chapter**
- **Exit Here** (open the item being watched and exit) and **Exit and Return** (go back to where you were). Closing
  the window with × is the same
- Titles play with picture and sound, and go on to their end action when they finish. The arrow keys and Enter
  are the remote control; Space pauses

### 12.2 Monitor (playing the build result)

**Window ▸ Monitor** (F9). **Open Build Output** in the Monitor reads the `VIDEO_TS` / `BDMV` of the written folder
and plays it. The remote control's arrow keys, Title / Menu, chapters and audio / subtitle work.
Before building, it shows the picture and sound of the selected timeline as they are (Space to play, with sound;
subtitles appear in the colours of the colour set).
DTS tracks are silent.

## 13. Building

**Window ▸ Build** (F2), or **File ▸ Build ▸ Folder…** / **Image…**.

| Field | Contents |
|---|---|
| Format / Output | DVD / Blu-ray, a folder (`VIDEO_TS` / `BDMV`) or an ISO image |
| Build | Starts the build. It saves the project and checks it first |
| Source | The current project (the Build panel writes only the project that is open) |
| Destination | When empty, the place shown in faint text (`<project name>_DVD` / `_BD` beside the project; `<project name>.iso` for an image). For a folder, naming an existing folder makes a `<disc name>` folder inside it (a place that already has `VIDEO_TS` / `BDMV` is built again in place). An image cannot be given a folder |
| Disc Info | The usage bar (**Refresh Usage** calculates it), the disc's name (also the ISO's volume label), the size, **DVD-ROM content** / **BD-ROM content** (**Browse…** chooses a folder; its files and folders go in the disc's root, beside `VIDEO_TS` / `BDMV`: a manual or photos to read on a computer. A folder build copies them, an image holds them. They take that much of the capacity from the video, and the line under the bar shows their size. **Clear** takes them off. A file must be under 4 GB, folders at most 7 deep) |
| Layer Break | DVD-9 only. Automatic / manual. Manual is set by timeline and position (timecode / frame / seconds, **Playhead**) |
| Region Codes | DVD only. All regions / custom (ticks for 1 to 8) |
| Project Check | The check results (the same rows as the check window) |
| Result | Errors, the number of titles / chapters / the size, **Simulate** (runs the VM after building to a folder) |

During a build a bar shows the progress and the time taken (also in the status bar, wherever the panel is).

- Converted media stays in `disccraft-cache` at the output (the next build skips the conversion). `--cache` on the
  command line keeps it elsewhere to share it
- An ISO is UDF + ISO 9660. On Windows a double click in Explorer mounts it
- A DVD-9 image with a layer break leaves a note in `<name>.layerbreak.txt` at the same place
- Nothing is burned to a disc. Use other software to burn
- How much of the disc is left, and what to do when it is not enough:
  [reading the disc's size as you work](#149-reading-the-discs-size-as-you-work)

## 14. Advanced techniques

The chapters before this one go in the order the work does. This chapter is the part that makes a disc
feel authored: scene-selection menus with pictures in the buttons, menus built out of the Library, routes
you set by hand, hidden buttons, highlight colours chosen button by button, motion menus with sound, and
reading the disc's size as you work. Each section is steps you can follow, with the limits next to them.

> Screenshots are placeholders for now. They are to be added under `docs/images/user-guide/`; until they
> are there, the picture shows as a broken link and the words below are all there is.

### 14.1 A scene-selection menu with pictures

Chapter points mark where scenes start. Link them to buttons and the viewer can jump to a scene instead of
scrubbing for it, and can step through the video with the remote control's Next and Previous Chapter buttons.

1. **File ▸ Import ▸ Timeline…** (Shift+Ctrl+T) makes an asset and a timeline for every video you choose,
   so the timelines do not have to be made one at a time. The first video becomes **First Play**.
2. Double-click a timeline in the Project panel to open it in the Timeline Viewer, and make sure the
   Monitor is showing (**Window ▸ Monitor**, F9).
3. Move the playhead to an obvious scene change and press **Timeline ▸ Add Chapter Point** (the `*` on the
   numeric keypad). Points are numbered in order and renumber themselves when one is added between two
   others.
4. Name the point in its properties. A chapter's name is normally the name of the button that will lead to
   it; **View ▸ Show Chapter Names** (Ctrl+8) puts the names on the timeline where you can read them.
5. Work through the video. **Add Chapter Points at Interval…** is for a video whose scenes are even
   (5 minutes by default).
6. Give the button a better picture than the scene's first frame. Move the playhead to a frame that stands
   for the scene and press the Monitor's **Poster Frame** button, or type a timecode into **Poster Frame**
   in the chapter's properties. The poster frame gets a flag of its own with the chapter's number; drag it
   to move it, and empty the field to go back to the chapter's own frame.
7. Make the menu: **Menu ▸ Create Chapter Index** for a button per chapter, or the Library's
   **Thumbnail Grid** preset for a menu of picture buttons. A button linked to a chapter shows that
   chapter's poster frame, or the chapter's own frame when it has none.

Things to know:

- Chapter 1 is always the first frame of the timeline. It cannot be moved or removed, and a new timeline
  comes with one.
- For MPEG-2 that goes on the disc as it is, a chapter point can only land on a GOP start. Drop the point
  where you want it and DiscCraft moves it to the nearest free GOP start, so a point in the middle of a
  GOP will not stay exactly where you left it. Trims snap the same way.
- A still thumbnail with no poster frame shows the chapter's first bright (non-black) frame, so a scene
  that opens on a fade-in from black still shows something in the button.
- A DVD takes 99 chapters in a title. A page of a chapter index holds 18 chapter buttons, or 6 when the
  index is made as multiple pages.
- With **Motion Buttons** on, a thumbnail button plays its video instead of holding one picture, from the
  linked chapter's start or its poster frame.

![Timeline Viewer with chapter points and poster frames](images/user-guide/chapter-poster.png)

### 14.2 Building a menu out of the Library

The Library holds DiscCraft's own menu, button, image, background and shape presets, and the ones you save
into it. A menu made this way is an ordinary menu afterwards; nothing marks where it came from.

1. In the **Library** panel, choose the kind (**Menus**) and the set (**General**, or **User** for your
   own), select a preset and press **New Menu**. It opens in the Menu Viewer and appears in the Project
   and Menus panels.
2. Name it (**Alt+Ctrl+R**) — "Menu #1", "Scene Selection", whatever you will recognise next month.
3. Switch the kind to **Buttons** and press **Place** once for every timeline the menu is to offer. The
   buttons land on the canvas.
4. With the **Selection tool** (V), move them roughly where they belong. Select several (Shift-click, or
   drag around them) and use **Object ▸ Distribute** and **Object ▸ Align** to space and line them up.
   With **Align Relative to Safe Area** on, the safe area frame is the reference instead of the canvas.
5. For a title in the same look as the buttons: select a button, **Ctrl+C** and **Ctrl+V**, then
   **Object ▸ Convert to Object**. The button goes and its text stays where it was, at the same size in
   the same face. Retype it with the **Text tool** (T). Copy and paste between menus the same way to carry
   the look along.
6. Select the title and choose **Object ▸ Drop Shadow…** (Shift+Ctrl+O). Tick **Drop Shadow** and
   **Preview**, set colour, opacity, angle, distance, size and spread, and press **OK**. The same dialog
   works on a button, an image or a shape.
7. For a way back, paste the text again, change it to "Main Menu" and press **Ctrl+B**
   (**Object ▸ Convert to Button**). This is how a button is made out of nothing.
8. Link each button ([14.7](#147-five-ways-to-link-a-button)), and set the menu's **End Action** and
   **Default Button**.
9. When a menu is worth keeping, **Menu ▸ Save Menu as Template…** (Ctrl+Alt+M) puts it in the Library's
   **User** set, and it is there in the next project too.

A background can be changed at any time: select a background in the Library and press **Place** or
**Set Background**. **Replace** swaps the open menu's design for another preset's and keeps the links by
button number.

![The Library panel and a menu built from its buttons](images/user-guide/library-menu.png)

### 14.3 Routing and numbering by hand

Routing is the path the highlight follows when the viewer presses the arrow keys. DiscCraft works out a
sensible path for you (the rules are in [Preferences ▸ Menus](#15-preferences) and are kept in the
project, so the disc routes the same wherever it is built). Set it by hand when the buttons are arranged
oddly, when a row should not wrap, or when a route should lead somewhere nobody would think to look.

1. Click the menu's background — not a button or text — so the Properties panel shows the menu's sheet,
   and turn off **Automatically Route Buttons** on the **Basic** tab.
2. **View ▸ Show Button Routing** (Ctrl+7) draws a cross on every button: the number in the middle is the
   button's number, and the number on each arrow is the button that arrow key moves to (0 is nowhere).
3. Set the numbers first. **Number** in the button's sheet moves the button in the list, and the arrows
   point at numbers, so numbering left to right and top to bottom makes the rest of the job obvious. The
   menu's **Offset** is where the remote control's number keys start counting.
4. Drag an arrow of the cross onto the button it should lead to. Releasing it over empty canvas clears
   that route. The directions you have not touched stay automatic.
5. Turn the routing display off and try it: **File ▸ Preview** (Alt+Ctrl+Space) and the arrow keys.

![The button routing cross](images/user-guide/routing.png)

### 14.4 A hidden button and an Easter egg

An Easter egg is an asset nobody finds by accident. On a disc it is reached through a button that is not
drawn — or not drawn until the highlight lands on it — and through a route nobody walks by default. There
are two kinds, and they differ only in which highlight is visible:

- one that shows its highlight when the cursor arrives, so a careful viewer can find it,
- one whose highlight is invisible too, so the only way to know you have found it is to press Enter.

Make one like this:

1. Make a menu (**File ▸ New ▸ Menu**, Ctrl+M, or a Library preset) and place two or three ordinary
   buttons and one image (**Library ▸ Images ▸ Place**). Do not overlap them.
2. Select the image and press **Ctrl+B**. That is the hidden button. Give it its link now — a timeline, a
   playlist, a slideshow — because a button with no link is reported by the check.
3. Click the background and turn off **Automatically Route Buttons**, so the route is yours to lay out.
4. In the **Layers** panel, open the button sets of the two buttons that are only there to be walked
   through and switch their eyes off. Their text and picture disappear; they stay buttons, and their
   highlights and links still work.
5. Select those two buttons and set **Highlight** to **Group 2**. Leave the visible button and the image
   button on **Group 1**.
6. **Menu ▸ Edit Menu Color Set…**, choose **Menu Default**, tick **Preview**, and set every opacity in
   Highlight Group 2 to 0 % — colours 1, 2 and 3, in both the selected and the activated column. The group 2
   buttons vanish completely.
7. In the same dialog, give Highlight Group 1 a colour you will notice (bright green is the classic) —
   that is the sign that the viewer has arrived. If the egg should stay dark even when it is found, set
   its opacity to 0 as well and the button is invisible even when selected.
8. Turn the routing display on and number the buttons: the visible one 1, the invisible ones 2 and 3, the
   hidden one last. Then drag the arrows so that the only way to the hidden button is a silly path —
   left, up, left — and every wrong key sends the viewer back to button 1.
9. Preview it with the arrow keys, not the mouse.

Notes:

- A hidden button is still a button. It holds its space, its highlight must not overlap another's, and it
  counts towards the 36 buttons of a menu.
- Keep it inside the frame. A button outside the frame is reported by the check and cannot be reached.
- An invisible button with no link is an error you will never see on the screen; run
  **File ▸ Check Project…** (Shift+Ctrl+L) before you build.
- To have the egg give the viewer their way back, link it to a timeline whose **End Action** is
  **Last menu**, or set the end action to the menu they came from.
- The other trick worth knowing: a button can link to a menu **with a button chosen**, so the menu opens
  with that button already highlighted.

![A menu with invisible buttons and the route between them](images/user-guide/hidden-button.png)

### 14.5 Highlight colours, button by button

A menu's highlights are subpictures: overlay pictures drawn on or around a button when the cursor arrives
(selected) and when the button is pressed (activated). A menu has a colour set — a normal group and three
highlight groups, and each group has three colours, each with a selected and an activated colour and
opacity. Every button says which group it uses.

1. Select the menu and choose **Menu Default** in **Color Set** instead of **Automatic**. Automatic
   invents colours for each menu, which is fine when there is one menu and messy when there are six.
2. **View ▸ Show Normal Subpicture / Show Selected Subpicture / Show Activated Subpicture**
   (Ctrl+3 / Ctrl+4 / Ctrl+5) shows each state on the canvas.
3. Select a button (click inside its border, off the routing cross) and change **Highlight** to another
   group. Buttons in the same group are highlighted together, which is what makes a row of "Chapter 1 / 2 / 3"
   buttons read as one group.
4. **Menu ▸ Edit Menu Color Set…** is where the colours are. Tick **Preview** and move the dialog aside so
   some buttons are still visible: change a colour or an opacity and the menu changes with it. **Use
   Selected Colors as Activated Colors** makes pressing a button look like arriving at it.
5. Most discs use one colour scheme throughout. Change Menu Default once and every menu that uses it
   follows.

To carry colours into another project, save the menu as a template (Ctrl+Alt+M) and open that template
there; a menu's colour set has no file of its own. Subtitle colour sets do (`.dccolors`,
[10.3](#103-appearance)).

![The Menu Color Set dialog](images/user-guide/color-set.png)

### 14.6 Motion menus, with sound and movement

A menu becomes a motion menu the moment its background is video. There are three ways to get one, in
order of how much trouble they are.

**A video behind an ordinary menu**

1. Import the video as an asset (**File ▸ Import ▸ Asset…**, Ctrl+I). It is the video, not a timeline,
   that goes behind a menu.
2. Select the menu, open the **Motion** tab of its sheet, and choose the video in **Background**. The
   first frame appears behind the buttons; the buttons and the graphics stay as they were.
3. Set **Loop** (how many seconds of the video the menu uses), **Loop #** (**None** for once only, a
   number, or **Infinite**) and, if the loop should not start at the beginning, **Loop Point**.
4. Set **Duration** (a menu is at most 254 seconds) and the **End Action** for when the time runs out.
   Without an end action a motion menu loops; a still menu with no end action stops on its last picture.
5. **Audio** is what plays while the menu is up. A still menu repeats with the length of its audio.

**Movement made in EffectCraft**

**Menu ▸ Create EffectCraft Composition…** writes the menu out as a layered PSD, makes an EffectCraft
project from it at the disc's frame rate and pixel shape, and puts a render of the menu's movie in the
render queue. Animate the layers there — a title that slides in, a background that breathes — and render.
When the movie appears, DiscCraft reads it and sets it as the menu's background automatically. Then:

- hide the layers you animated in the **Layers** panel, so on the disc they start empty and arrive with
  the movie instead of being drawn twice,
- set **Loop #** to **None**, so the movie plays when the menu opens and stops on its last frame instead
  of starting again,
- watch it after building (**Window ▸ Monitor**, F9, **Open Build Output**). In the Menu Viewer and the
  Project Preview a motion menu shows the background's first frame — the same as Encore before you press
  Render.

**Movement in the buttons**

**Motion Buttons** on the **Motion** tab makes the menu's thumbnail buttons play their video. It costs bit
rate, and it is the kind of motion menu that older players mind most, so try it on a real player before a run
of discs.

![The Motion tab of a menu](images/user-guide/motion-tab.png)

### 14.7 Five ways to link a button

| Way | How | Use it when |
|---|---|---|
| Drag from the Project panel | Drag a timeline, slideshow, playlist or chapter playlist onto the canvas. It becomes a button there, named after the item and linked to it | You are making the menu and the item is to hand |
| The **Link** field | Select the button and choose the target in its sheet. A button's list has the timelines' chapters as well as the timelines | You are setting several buttons in order |
| The pick whip | Press **▶** at the right of the field and click the item in the Project panel | The target is far down a long list |
| **Object ▸ Specify Link…** (Ctrl+L) | A dialog with every target grouped by kind. For buttons it also sets the audio and subtitle the button switches to | You want the whole project in one list, streams included |
| The Flowchart | Drag from a button's port to another node, or right-click a node ▸ **Specify Link…** | You are drawing the navigation and want to see where it goes |

The same five ways set an end action, an override and a remote menu. Two targets are worth remembering:
**Stop** ends playback, and **Last menu** goes back to the menu the viewer came from — that is what makes
"Play All" behave.

### 14.8 The Flowchart as a map of the disc

The Flowchart (**Window ▸ Flowchart**, F5) is where the navigation can be read instead of hunted for. The
disc node is at the top with its **First Play**, **Title button** and the disc's **Override**; below it,
menus, timelines, slideshows and playlists as boxes with their links as lines.

- Drag a box out of the **unlinked items** row at the bottom onto the disc node's **First Play** port to
  make it the first thing the disc does.
- Drag from a button's port to a menu, timeline, slideshow or playlist to link it. Drag from an end
  action's port to say where the item goes when it finishes. Every line has an arrow head at its target,
  so a menu that leads back up to a higher menu reads as a loop rather than a tangle, and a box with no
  line into it is an orphan.
- **Auto Layout** / **Auto Layout All** tidy the boxes; a box you have moved by hand stays where you put
  it until you ask for **Auto Layout** on it.
- Right-click a node for **Open**, **Specify Link…**, **Rename…**, **Set as First Play**, and
  **Preview from Here** (the preview starts at that item).
- **View ▸ Show All Thumbnails** puts a picture in every box, which is the fastest way to find the menu
  you meant.

A disc that plays badly is usually one with a missing end action or an orphan. The check finds both
([11](#11-checking-the-project)), and the Flowchart shows them as boxes with no line into them.

![The Flowchart with linked menus and timelines](images/user-guide/flowchart.png)

### 14.9 Reading the disc's size as you work

Bit budgeting is deciding how much picture quality fits on the disc. The disc's size is fixed; the video's
bit rate is what gives.

- The **Build** panel's **Disc Info** bar is the running total (**Refresh Usage** recalculates it). Small
  projects need nothing more than a glance at it now and then.
- A DVD holds everything at once: video, audio, subtitles and the menus' own video. The ceiling is
  9.8 Mbps for a title, and the check reports a timeline that passes it (**Timeline Bitrate Too High**)
  when the video is used as it is.
- When the video is converted, DiscCraft chooses the bit rate to fill the disc, up to 9.8 Mbps by default.
  Longer content means a lower rate, and a lower rate shows first in fast motion and in film grain.
- If the disc is over, the choices are the same three they always were: cut content, lower the rate, or
  use a bigger disc (DVD9, BD50…). DVD-9 adds the layer break, which is best placed at a scene change —
  set it by timeline and timecode under **Layer Break** ([13](#13-building)).
- Keep the audio honest too. Converted audio is AC-3 192 kbps; AC-3, WAV and DTS go on as they are, so a
  1.5 Mbps WAV track or a 1.5 Mbps DTS track is 1.5 Mbps taken off the video.
- A rough figure for a DVD5: about an hour at the top rate, and two hours at a rate most people will not
  complain about. Two hours of busy action on one disc is where the quality starts to suffer.

### 14.10 Blu-ray

The same project can be built as a Blu-ray. **File ▸ Project Settings…** or the **Disc** panel switches the
authoring mode, and the **Disc** panel also chooses the video's size, SD (MPEG-2) or HD (H.264).

- Blu-ray is HDMV only (no BD-J), and the menus work as they do on a DVD.
- A **pop-up menu** is the Blu-ray's version of a remote menu: make it on the menu's **Pop-up** tab, and
  set the timeline's **Remote Menu** to it. The viewer can call it up in the middle of a title.
- HD (1080p, 720p) is H.264, and clips are joined at IDR pictures, so trims come out exactly where you
  cut them — unlike a DVD, where a converted clip is cut at a GOP start.
- A DTS file cannot go on a Blu-ray yet.
- Subtitles on an HD or PAL Blu-ray need 5 frames between one subtitle and the next on the same track; the
  check reports it (**Subtitle Spacing**).
- Build it the same way (**File ▸ Build ▸ Folder… / Image…**); the folder is `BDMV` instead of `VIDEO_TS`.
  The Project Preview and the Monitor play a built Blu-ray, pop-up menu included.

### 14.11 What is not here yet

So that you are not looking for it:

| Encore has | DiscCraft |
|---|---|
| Burning to a disc, DDP export | Not there. DiscCraft writes a folder or an ISO |
| Copy protection (CSS, AACS) | Not there. Only the region code flags |
| BD-J menus | Not there. Blu-ray is HDMV |
| Motion menus rendered inside the application | Not there. The Menu Viewer and the Project Preview show the background's first frame; render in EffectCraft, or build and watch the result |
| Highlight colour sets shared between projects as a file | Not there for menus (subtitle colour sets have `.dccolors`). Save the menu as a template instead |
| GOP headers as marks on the ruler, and Alt-stepping between them | Not there. Chapter points and trims snap to GOP starts automatically |

## 15. Preferences

**Edit ▸ Preferences…** (Ctrl+K). Categories on the left, settings on the right.

| Category | Contents |
|---|---|
| General | The default television standard (for new projects). Kept in `workspace.json` |
| Timelines | Still duration (the slide duration of new slideshows; 6 seconds by default), Subtitle duration (the length of a subtitle added in the Monitor; 2 seconds by default), Audio language / Subtitle language (the language of new tracks; empty uses the disc's audio language). Kept in `workspace.json` |
| Menus | Button routing (movement up, down, left and right, how and by which rule to wrap, leaving out "Next" and "Previous" buttons), the default for syncing button names. **Kept in the project** (the routing is the same whichever computer builds it) |
| Appearance | Display language (日本語 / English). **Window ▸ Language** switches it too. Until you choose one, every start follows the operating system's display language (Japanese on a Japanese system; English on an English system and on one in any other language) |

## 16. Keyboard shortcuts

`Ctrl` is `Cmd` on macOS. Some (Ctrl+Z and others) do not act while you type text.

### 16.1 Files and projects

| Key | Operation |
|---|---|
| Ctrl+N / Ctrl+O / Ctrl+S / Shift+Ctrl+S | New project / Open / Save / Save As |
| Shift+Ctrl+W | Close Project |
| Ctrl+I | Import ▸ Asset… |
| Shift+Ctrl+I / Shift+Ctrl+K / Shift+Ctrl+U | Import ▸ Menu… / Pop-up Menu… / Menu Page… |
| Shift+Ctrl+T / Shift+Ctrl+G | Import ▸ Timeline… / Slideshow… |
| Shift+Ctrl+L | Check Project… |
| Alt+Ctrl+Space | Preview |
| Ctrl+Q | Exit |

### 16.2 Creating and editing

| Key | Operation |
|---|---|
| Ctrl+M / Ctrl+T / Ctrl+G | New Menu / New Timeline / New Slideshow |
| Ctrl+Z / Shift+Ctrl+Z | Undo / Redo |
| Ctrl+X / Ctrl+C / Ctrl+V / Ctrl+D | Cut / Copy / Paste / Duplicate |
| Delete | Clear |
| Ctrl+A / Shift+Ctrl+A | Select All / Deselect All |
| Alt+Ctrl+R | Rename… |
| Ctrl+E | Edit Original |
| Shift+Ctrl+M | Edit Menu in PhotoCraft |
| Ctrl+K | Preferences… |

### 16.3 Objects

| Key | Operation |
|---|---|
| Ctrl+B | Convert to Button |
| Ctrl+L | Specify Link… |
| Shift+Ctrl+O | Drop Shadow… |
| Alt+Ctrl+H / Alt+Ctrl+V | Flip Horizontal / Flip Vertical |
| Ctrl+] / Shift+Ctrl+] | Bring Forward / Bring to Front |
| Ctrl+[ / Shift+Ctrl+[ | Send Backward / Send to Back |
| Ctrl+Alt+M | Save Menu as Template… |
| V / A / M / R / T / Y / Z | Selection / Direct Select / Move / Rotate / Text / Vertical Text / Zoom tool |

### 16.4 View and playback

| Key | Operation |
|---|---|
| Space | Play / stop |
| + / - / \\ | Zoom In / Zoom Out / Fit in Window |
| ← / → | One frame back / forward |
| `*` (numeric keypad) | Add Chapter Point |
| Ctrl+3 / Ctrl+4 / Ctrl+5 | Show the normal / selected / activated subpicture highlight |
| Ctrl+6 / Ctrl+7 / Ctrl+8 | Show Safe Areas / Button Routing / Chapter Names |
| Ctrl+: / Shift+Ctrl+: / Alt+Ctrl+: | Show Guides / Snap to Guides / Lock Guides |
| Shift+Alt+Ctrl+: | New Guide… |

### 16.5 Window

| Key | Operation |
|---|---|
| F1 | DiscCraft Help… (this manual) |
| F2 / F4 / F5 | Build / Check Project / Flowchart |
| F6 / F7 / F8 / F9 | Layers / Library / Menus / Monitor |
| F11 / F12 / Ctrl+F3 | Project / Properties / Timelines |

## 17. Command line and scripts

Everything the GUI does can also be done from `disccraft-cli` (building, inspecting a project, running scripts).

```
cargo run -p disccraft-cli -- commands [--json]
cargo run -p disccraft-cli -- exec <project.dcproj> <command> [params-json]
cargo run -p disccraft-cli -- run <project.dcproj> <script.jsonl>
cargo run -p disccraft-cli -- inspect <project.dcproj> [--json]
cargo run -p disccraft-cli -- build <project.dcproj> --out <dir>
cargo run -p disccraft-cli -- build <project.dcproj> --iso <file> [--cache <dir>] [--audio-language ja]
```

- `commands` lists the engine's commands (id, label, place in the menus, shortcut, parameters). `--json` for JSON
- `exec` runs one command and prints the result as JSON. It saves the project when it changed
- `run` runs a JSON Lines script. One line is one command, `{"command": "...", "params": {...}}`.
  Lines starting with `#` or `//` are ignored. A script may be up to 16 MB
- `inspect` prints a summary of the project (`--json` gives the JSON of `project.inspect`)
- `build` makes a folder with `--out` and an image with `--iso`. `--cache` names where converted media is kept;
  `--audio-language` gives the language (ISO 639-1) of audio that has none

## 18. Using it from an AI agent (MCP)

`disccraft-cli mcp` runs as an MCP server (JSON-RPC over standard input and output). It has three tools.

| Tool | Contents |
|---|---|
| `disccraft_commands` | The list of commands (id, label, menu, shortcut, parameters) |
| `disccraft_execute` | Runs one command with its parameters |
| `disccraft_inspect` | Inspects the current project and gives the check results |

In your MCP client, register `<repository path>/target/release/disccraft-cli` as the command with `mcp` as its
argument (connected over stdio).

## 19. Limits and rules

| Item | Limit |
|---|---|
| Buttons | 36 in a menu |
| Menu duration | 254 seconds |
| Video clips in a timeline | 99 |
| Chapters | 99 in a title (DVD) |
| DVD titles | 99 |
| Audio tracks | 8 |
| Subtitle tracks | 32 |
| Chapter index | 18 buttons a page (6 a page for a multi-page index) |
| Slide duration | 6 seconds by default. Transitions 0.1 to 10 seconds (2 by default) |
| Subtitle trim | 0.4 seconds at the shortest (13 frames NTSC / 11 PAL) |
| Subtitle text | Up to 10 lines at the standard NTSC size inside the title-safe frame |
| Gap between subtitles | 5 frames or more on a Blu-ray (HD / PAL); no limit on a DVD |
| DVD bit rate | 9.8 Mbps for video, audio and subtitles together |
| Sizes | DVD5 4.7 GB, DVD9 8.5 GB, BD25 / BD50 / BD100 / BD128 |
| Drop | 500 files at a time |
| Recent projects | 10 |
| Workspaces | 32 (names up to 64 characters) |

Other rules:

- Button highlights do not overlap on the disc (they may overlap on the editing screen)
- A timeline has a video as its first clip
- On a DVD, video clips that need a conversion are not cut at the trimmed place (they are cut at GOP starts)
- A 16:9 menu also uses the same 720×480 frame in SD and is shown stretched sideways (an HD Blu-ray uses a
  1920×1080 frame). The aspect ratio of a DVD's menus is the same for the whole disc
- Converted audio is Dolby Digital (AC-3) 192 kbps. AC-3 and WAV media are used as they are. DTS (`.dts`) is used
  as it is on a DVD only
- MPEG-2 video that fits a DVD (720 / 704 / 352 wide) is not converted. The clips of one title must have the same
  width

## 20. When something does not work

| Symptom | What to do |
|---|---|
| Media is "offline" | Right-click in the Project panel ▸ **Replace Asset…** and give the new place |
| The build stops with "there are problems" | Open **Check Project…** and read **Name** and **Problem**. When it cannot be fixed, choose **Ignore and Continue** in the dialog |
| A menu does not play | Check First Play, the button links and the end actions in the Flowchart |
| A button does not respond | See whether highlights overlap (red frame); **View ▸ Show Button Routing** shows where the arrows lead |
| A hidden button cannot be reached | Its route has to come from a button that can be reached: [a hidden button and an Easter egg](#144-a-hidden-button-and-an-easter-egg) |
| No sound | Choose the track you hear with the speaker button in the Monitor's track header. DTS tracks are silent in the Monitor and the preview (they do go on the disc). On Linux the ALSA development package (`libasound2-dev` or the like) is needed |
| An EffectCraft project does not show the picture you expected | Choose the composition to use under **Composition** in the asset's properties |
| Garbled text | Switch the display language in **Window ▸ Language**. Subtitles are read as UTF-8 / UTF-16 / Shift_JIS |
| The layout is a mess | **Window ▸ Workspace ▸ Reset “name”** |
| The written disc does not play | Open the written folder or image in a software player (VLC and the like) and see whether ffprobe reads its streams, to tell where it stops |
| You want to report a problem | **Copy** in **Help ▸ System Compatibility Report…** gives your setup, and **Help ▸ Reveal Log Files…** gives the record of what was done since DiscCraft started (the log); add them to **Help ▸ Report an Issue…**. When DiscCraft stops unexpectedly, the log of that moment is left in the log folder too |
| You want to know whether there is a newer version | **Help ▸ Check for Updates…** shows this version and opens the releases on GitHub (DiscCraft does not go online by itself) |
| A panel cannot be found | The **Window** menu has the name of every panel (one closed with × opens again from here) |
