# 1. About DiscCraft

[Contents](README.md) · Next: [2. Getting Started](02-getting-started.md#2-getting-started)

> **In this chapter**
>
> - Learn about the discs (DVD and Blu-ray) you can create with DiscCraft
> - Understand how to read this manual
> - Find sections based on what you want to do

## 1.1 What You Can Do with DiscCraft

DiscCraft is authoring software for creating DVD-Video and Blu-ray (BDMV) discs from video, audio, and still images. It
lets you assemble menus, chapters, multiple audio tracks and subtitles, and navigation. It automatically converts assets
that do not meet specifications and writes the output to a folder or an ISO image.

What you can do:

- Create DVD and Blu-ray discs. You can write directly to a disc, or export to a folder (`VIDEO_TS` / `BDMV`) or an ISO
  image.
- Create menus. This includes still-image menus, motion menus with animated backgrounds, and menus created in Photoshop
  (PSD).
- Concatenate videos into a single title, adding chapters, audio tracks in different languages, and subtitles.
- Create photo slideshows and playlists that play multiple items consecutively.
- Define what plays when the disc is inserted, where buttons link to, and where playback goes after it ends.
- Test your project as if it were a finished disc before building (Project Preview).
- Automatically convert assets with incompatible formats. Video (MPEG-2) and audio (AC-3 / WAV / DTS) that already match
  DVD specifications are included without conversion.
- Perform the same operations not only from the screen but also via command line or AI agents (MCP).

What you cannot do (at this time):

- Writing to discs on macOS and Linux (you can create folders and ISO images, so use other software for writing). On
  Windows, you can write directly to a disc from the Build panel.
- Copy protection (CSS, AACS, etc.). You can only set the Region Code.
- Blu-ray menus that run on Java programs (BD-J). DiscCraft's Blu-ray menus are built like DVD menus (HDMV).
- Converting audio to DTS / Dolby TrueHD / DTS-HD (you can include DTS files as they are).
- Editing video itself (perform editing in software like FilmCraft, then use DiscCraft for disc authoring).
- 4K (Ultra HD Blu-ray)

The video formats for Blu-ray (SD and HD, codecs) are summarized in [Chapter 16](16-bluray.md#16-making-a-blu-ray-disc).

Screen names and operations align with Adobe Encore (DVD and Blu-ray authoring software; development has ended). If you
have used Encore, you can create projects using the same menus and panels. Differences are listed in
[17.10 Not Yet Available](17-advanced.md#1710-not-yet-available).

## 1.2 How to Read This Manual

| Notation | Meaning |
|---|---|
| **File ▸ Import ▸ Asset…** | Follow the menu items from left to right. Bold text indicates names shown on the screen. |
| Ctrl+I | Press I while holding down Ctrl. |
| `.dcproj` | File or folder names, and characters you type. |
| [5.1 How to Import](05-assets.md#51-how-to-import) | Links to other chapters or sections (clickable in PDF). |

Each chapter begins with a box describing what you can do in that chapter. There are two types of boxes within the text.

> **Tip**
>
> Information that makes your work easier.

> **Details**
>
> Specific rules or matters related to disc specifications. You can skip these when reading for the first time.

The pictures of the screen were taken on Windows with the interface in English. The names in the Japanese interface are
given in the [Glossary](24-glossary.md#24-glossary).

## 1.3 Finding a Task

<!-- pages -->

| What you want to do | Where to read |
|---|---|
| Try making one disc first | [3. Making Your First DVD](03-tutorial.md#3-making-your-first-dvd) |
| Create or save a project | [2.2 New Project](02-getting-started.md#22-new-project), [2.3 Saving](02-getting-started.md#23-saving) |
| Import video, photos, and music | [5.1 How to Import](05-assets.md#51-how-to-import) |
| Concatenate videos into a single title | [6.2 Multiple Video Clips](06-timelines.md#62-multiple-video-clips) |
| Add chapters | [6.3 Chapter Points](06-timelines.md#63-chapter-points) |
| Include audio or subtitles in different languages | [6.4 Tracks](06-timelines.md#64-tracks), [11. Subtitles](11-subtitles.md#11-subtitles) |
| Create a photo slideshow | [7. Making Slideshows](07-slideshows.md#7-making-slideshows) |
| Play main feature and extras consecutively | [8.1 Playlist](08-playlists.md#81-playlist) |
| Create menus and place buttons | [9.1 How to Create Them](09-menus.md#91-how-to-create-them), [9.2 Buttons](09-menus.md#92-buttons) |
| Create a menu for selecting chapters (Scene Selection) | [8.3 Chapter Index](08-playlists.md#83-chapter-index), [17.1 Scene Selection Menus with Images](17-advanced.md#171-scene-selection-menus-with-images) |
| Create a menu with an animated background | [9.5 Backgrounds and Motion Menus](09-menus.md#95-backgrounds-and-motion-menus) |
| Use menus created in Photoshop | [9.6 Rebuilding in PSD](09-menus.md#96-rebuilding-in-psd) |
| Define what plays first when the disc is inserted | [10.1 First Play and Title Button](10-navigation.md#101-first-play-and-title-button) |
| Return to menu after playback ends | [6.5 End Action, Override, Remote Menu](06-timelines.md#65-end-action-override-remote-menu) |
| Define where buttons link to | [10.3 Types of Links](10-navigation.md#103-types-of-links), [17.7 Five Ways to Link Buttons](17-advanced.md#177-five-ways-to-link-buttons) |
| Find errors before building | [12. Checking the Project](12-check.md#12-checking-the-project) |
| Test the disc without building | [13.1 Project Preview (Preview Without Building)](13-preview.md#131-project-preview-preview-without-building) |
| Create folders or ISO images | [14.2 Destination](14-build.md#142-destination) |
| Write to a disc | [14.7 Writing to a Disc](14-build.md#147-writing-to-a-disc) |
| Check remaining disc capacity | [14.3 Disc Info](14-build.md#143-disc-info), [17.9 Monitoring Disc Size While Working](17-advanced.md#179-monitoring-disc-size-while-working) |
| Print a disc label | [15. Making a Disc Label](15-labels.md#15-making-a-disc-label) |
| Create a Blu-ray | [16. Making a Blu-ray Disc](16-bluray.md#16-making-a-blu-ray-disc) |
| Operate quickly with the keyboard | [19. Keyboard Shortcuts](19-shortcuts.md#19-keyboard-shortcuts) |
| Automate repetitive tasks with commands | [20. Command Line and Scripts](20-command-line.md#20-command-line-and-scripts) |
| Things are not working as expected | [23. Troubleshooting](23-troubleshooting.md#23-troubleshooting) |
| Look up word meanings | [24. Glossary](24-glossary.md#24-glossary) |

<!-- /pages -->
