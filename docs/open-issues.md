# Open Issues and Planned Work

English | [日本語](open-issues.ja.md)

What DiscCraft does not do yet, what is planned, and where help with testing is wanted. As of 2026-11-20 (0.1.0
alpha). Reports of what works and what does not are welcome.

What the status words mean:

| Status | Meaning |
|---|---|
| Planned | Can be worked on now |
| Large | Planned, but a large piece of work |
| Needs a decision | Waiting for a decision (policy, cost, or a choice between approaches) |
| Upstream | Depends on the libraries DiscCraft uses (craft-libs) |
| Awaiting merge | Works in DiscCraft through a fork of craft-libs; waiting for the pull requests to craft-libs to be merged (DiscCraft then goes back to craft-libs itself) |
| Needs hardware | Needs testing on real players or drives |
| On hold | Waiting for public documentation (DiscCraft is written from public specifications only) |
| Out of scope | Not planned |

## 1. Writing to disc

DiscCraft writes VIDEO_TS / BDMV folders and ISO images. Burning them to a disc is left to other software for now.

| Status | Item |
|---|---|
| Large | Burn a build to a DVD or Blu-ray on Windows (the operating system's disc writing service): choice of drive, speed and number of copies, as the Build panel's "Disc" output |
| Large | Dual-layer discs (DVD-9, BD-50): check that the layer change falls where the image marks it |
| Large | Burning on macOS and Linux |
| Needs a decision | DDP export, double-sided discs |
| Needs hardware | All of the above needs testing with a writer drive and blank discs |

## 2. Blu-ray and HD

| Status | Item |
|---|---|
| On hold | Blu-ray parental control. It works differently from a DVD's (the player keeps an age, not a level from 1 to 8), and the public documentation found so far does not say how a disc's navigation commands read the player's setting |
| Large | Coding interlaced sources whose two fields move apart as fields. The MPEG-2 encoder codes interlaced pictures, and since a fix to its motion search they code sharp synthetic interlaced material about 3 dB better than frames; interlaced material made from 59.94p video still codes 0.2 to 0.4 dB better as frames, so conversions do not use them yet. Next: field prediction, and measuring with real camera footage |
| Large | The rest of HD MPEG-2 that goes on the disc as it is: field pictures |
| Awaiting merge | Decoding interlaced H.264 (camcorder and recorder 1080i): works through a fork of craft-libs that carries its pull requests [#1992](https://github.com/storytold/craft-libs/pull/1992) and [#2017](https://github.com/storytold/craft-libs/pull/2017) (field pictures and MBAFF), which are not merged yet |
| Awaiting merge | Encoding 1080i as H.264: works through the fork; sent to craft-libs as [issue #2535](https://github.com/storytold/craft-libs/issues/2535) and [pull request #2537](https://github.com/storytold/craft-libs/pull/2537) (a draft stacked on #1992) |
| Needs a decision | Using an ffmpeg the user has installed, optionally, for what DiscCraft's own decoders cannot read (DiscCraft does not use ffmpeg today) |
| Large | B pictures in MPEG-2 (DVD and 1080i Blu-ray). DiscCraft's own encoder makes I and P pictures only. They can be added from the standard, so this does not wait for craft-libs (craft-video's MPEG-2 encoder has B pictures, but measured at the same bitrate its pictures were worse) |
| Needs hardware | HRD / SEI details that some hardware Blu-ray players may need |
| Needs a decision | 4K (Ultra HD Blu-ray) |
| Out of scope | DTS-HD and Dolby TrueHD as they are |

## 3. DVD

| Status | Item |
|---|---|
| On hold | Parental masks per country: the public documentation found so far gives the tables' layout but not how a player matches the masks to choose what plays. The parental levels DiscCraft writes now (the disc's and each title's) apply in every country |
| Large | The DVD standard's parental blocks (cells chosen by level). A title can already name what plays in its place when the player's level is too low |
| Needs a decision | Multi-angle titles (Encore does not have them) |
| Upstream | Opening VCD-style MPEG-1 system streams in the media reader (DiscCraft works around it) |

## 4. Import and speed

| Status | Item |
|---|---|
| Out of scope | Decoding a source in parts side by side: measured, splitting the work into more threads made the conversion slower (the encoder already uses several cores) |
| Large | Conversion on the GPU (hardware encoders, scaling and colour on the GPU). The licences are checked and all of them can be used; next, trying whether Windows Media Foundation's hardware H.264 keeps to what a Blu-ray needs |
| Awaiting merge | A large MKV's frame index is kept in the media cache, so it is read through once only (before, 15 to 25 seconds for 18 GB at every start). It works through the fork; sent to craft-libs as [issue #2534](https://github.com/storytold/craft-libs/issues/2534) and [pull request #2536](https://github.com/storytold/craft-libs/pull/2536). MP4 files open quickly anyway |
| Awaiting merge | A transport stream's index (`.m2ts` / `.ts` / `.mpg` / `.vob`) is kept in the media cache, so the file is read through once only (before, the whole file was read at every start: 1.8 seconds for 3.9 GB, then under 0.01 seconds the next time, with the file in the OS cache). It works through the fork; sent to craft-libs as [issue #2540](https://github.com/storytold/craft-libs/issues/2540) and [pull request #2542](https://github.com/storytold/craft-libs/pull/2542) (written in a session kept apart from DiscCraft) |

## 5. Timeline, menus and panels

| Status | Item |
|---|---|
| Needs a decision | Threading text from a frame's overset "+" into another frame. Encore's documentation only has the text reflow inside its box when the box is made bigger with a text tool; no threading into another frame was found. Whether to add what Encore does not have |
| Needs a decision | Editing metadata and writing it back (DiscCraft never changes the source files) |

## 6. Releases and packages

| Status | Item |
|---|---|
| Planned | The x86_64 Flatpak is missing from 0.1.0-alpha.6 (its build failed); the fix is in the next release |
| Needs hardware | Installing with the `.msi`, the Arm64 build, macOS, and the Linux packages (AppImage, Flatpak, deb, rpm, Arch) have not been tried on real machines |
| Needs a decision | Publishing the Flatpak as an OCI image |
| Needs a decision | Signing the programs (Windows SmartScreen and macOS Gatekeeper ask for confirmation the first time) |
| Out of scope | Copy protection (CSS, AACS) |

## 7. Checks on real players

| Status | Item |
|---|---|
| Needs hardware | Discs played on hardware DVD and Blu-ray players |
| Needs hardware | VLC draws Blu-ray highlights a little wider than placed, and draws only the highlighted part of a DVD menu's subpicture; these look like VLC's own behaviour |
| Needs hardware | ffmpeg reports an error at the start of a Blu-ray chapter playlist that starts from a middle chapter |
