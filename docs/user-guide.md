# DiscCraft User Manual

English | [日本語](user-guide.ja.md)

How to use DiscCraft, in the order the work goes. Names of commands and
panels are the ones the English interface shows.

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
14. [Preferences](#14-preferences)
15. [Keyboard shortcuts](#15-keyboard-shortcuts)
16. [Command line and scripts](#16-command-line-and-scripts)
17. [Using it from an AI agent (MCP)](#17-using-it-from-an-ai-agent-mcp)
18. [Limits and rules](#18-limits-and-rules)
19. [When something does not work](#19-when-something-does-not-work)

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
- 1080i and MPEG-2 HD, 4K (Ultra HD Blu-ray)

Blu-ray video is SD (720×480 / 720×576) MPEG-2 or HD (1080p, 720p) H.264.
HD 1080p at 25 / 29.97 / 50 / 59.94 is outside the Blu-ray specification and will be removed.

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
- The default television standard can be changed in [Preferences](#14-preferences)
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

### 4.2 Files that can be read

| Kind | Extensions |
|---|---|
| Importable as assets | mp4, mov, m4v, avi, mkv, m2v, wav, ac3, dts, mp3, aac, flac, png, jpg, jpeg, bmp, tif, tiff, psd, pcraft, svg, srt |
| Video that can become a timeline | mp4, mov, m4v, avi, mkv, mxf, m2v, mpg, mpeg, vob, m2ts, ts |
| Subtitles | srt, vtt, txt |
| Menu designs | psd, pcraft |
| Craft documents | `.scraft` (a SoundCraft session → audio), `.ecproj` (an EffectCraft project → video), `.fcproj` (a FilmCraft sequence → a timeline) |

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
- Media in use cannot be deleted. Taking an item out of use sets the links to it back to Not Set

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

A DVD takes up to 99 chapters in a title.

### 5.4 Tracks

**Timeline ▸ Add Audio Track… / Add Subtitle Track…** add one track at the bottom;
**Remove Audio Track / Remove Subtitle Track** remove the bottom one. The **Disc** panel (the same as the properties
with nothing selected) sets the audio track heard and the subtitle shown by default. In the Monitor, the speaker
button in a track's header chooses the track you hear.

The limits are 8 audio tracks and 32 subtitle tracks.

### 5.5 End action, override, remote menu

The timeline's sheet in the Properties panel has the same three link fields as Encore.

| Field | Meaning |
|---|---|
| End Action | Where to go after it has played to the end (a menu, another timeline, a slideshow, a playlist, nothing) |
| Override | Where to go after the end action's target has finished (a flow like warning → feature → extras menu) |
| Remote Menu | The menu the remote control's Menu button shows (the pop-up menu on a Blu-ray) |

There are three ways to set a link.

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
[Preferences ▸ Menus](#14-preferences) (kept in the project).

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

### 8.5 Background and motion menus

Set in the menu's sheet of the Properties panel (**Basic / Motion / Pop-up**).

- The background is a colour / an image / a timeline (video). With video it becomes a motion menu
- Loop count and loop point, duration (a menu is at most 254 seconds)
- **End Action** (where to go when time runs out; Menu ▸ Set End Action… opens it too) and **Override**
- The **Pop-up** sheet makes it a pop-up menu and sets its background timeline / background frame (Blu-ray)

**Menu ▸ Create EffectCraft Composition…** makes an EffectCraft project from the menu's layers; the rendered movie
can be the menu's background (a motion menu).

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
| Disc Info | The usage bar (**Refresh Usage** calculates it), the disc's name (also the ISO's volume label), the size |
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

## 14. Preferences

**Edit ▸ Preferences…** (Ctrl+K). Categories on the left, settings on the right.

| Category | Contents |
|---|---|
| General | The default television standard (for new projects). Kept in `workspace.json` |
| Menus | Button routing (movement up, down, left and right, how and by which rule to wrap, leaving out "Next" and "Previous" buttons), the default for syncing button names. **Kept in the project** (the routing is the same whichever computer builds it) |
| Appearance | Display language (日本語 / English). **Window ▸ Language** switches it too. Until you choose one, every start follows the operating system's display language (Japanese on a Japanese system; English on an English system and on one in any other language) |

## 15. Keyboard shortcuts

`Ctrl` is `Cmd` on macOS. Some (Ctrl+Z and others) do not act while you type text.

### 15.1 Files and projects

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

### 15.2 Creating and editing

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

### 15.3 Objects

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

### 15.4 View and playback

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

### 15.5 Window

| Key | Operation |
|---|---|
| F2 / F4 / F5 | Build / Check Project / Flowchart |
| F6 / F7 / F8 / F9 | Layers / Library / Menus / Monitor |
| F11 / F12 / Ctrl+F3 | Project / Properties / Timelines |

## 16. Command line and scripts

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

## 17. Using it from an AI agent (MCP)

`disccraft-cli mcp` runs as an MCP server (JSON-RPC over standard input and output). It has three tools.

| Tool | Contents |
|---|---|
| `disccraft_commands` | The list of commands (id, label, menu, shortcut, parameters) |
| `disccraft_execute` | Runs one command with its parameters |
| `disccraft_inspect` | Inspects the current project and gives the check results |

In your MCP client, register `<repository path>/target/release/disccraft-cli` as the command with `mcp` as its
argument (connected over stdio).

## 18. Limits and rules

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

## 19. When something does not work

| Symptom | What to do |
|---|---|
| Media is "offline" | Right-click in the Project panel ▸ **Replace Asset…** and give the new place |
| The build stops with "there are problems" | Open **Check Project…** and read **Name** and **Problem**. When it cannot be fixed, choose **Ignore and Continue** in the dialog |
| A menu does not play | Check First Play, the button links and the end actions in the Flowchart |
| A button does not respond | See whether highlights overlap (red frame); **View ▸ Show Button Routing** shows where the arrows lead |
| No sound | Choose the track you hear with the speaker button in the Monitor's track header. DTS tracks are silent in the Monitor and the preview (they do go on the disc). On Linux the ALSA development package (`libasound2-dev` or the like) is needed |
| An EffectCraft project does not show the picture you expected | Choose the composition to use under **Composition** in the asset's properties |
| Garbled text | Switch the display language in **Window ▸ Language**. Subtitles are read as UTF-8 / UTF-16 / Shift_JIS |
| The layout is a mess | **Window ▸ Workspace ▸ Reset “name”** |
| The written disc does not play | Open the written folder or image in a software player (VLC and the like) and see whether ffprobe reads its streams, to tell where it stops |
| A panel cannot be found | The **Window** menu has the name of every panel (one closed with × opens again from here) |
