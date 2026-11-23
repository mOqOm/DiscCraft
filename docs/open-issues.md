# Open Issues and Planned Work

English | [日本語](open-issues.ja.md)

What DiscCraft does not do yet, what is planned, and where help with testing is wanted. As of 2026-11-23 (0.1.0
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

DiscCraft writes VIDEO_TS / BDMV folders and ISO images, and on Windows also writes them to a disc through the Build panel's "Disc" output (M9.35).

| Status | Item |
|---|---|
| Large | Dual-layer discs (DVD-9, BD-50): check that the layer change falls where the image marks it |
| Large | Burning on macOS and Linux |
| Out of scope | Double-sided discs (the Side setting of Encore's Build panel) |
| Needs hardware | Test the Windows writing (M9.35) on the other kinds of blank disc (DVD+R, DVD-RW, DVD+RW, BD-R, BD-RE). A DVD-R was written, read back the same as its image, and played in PowerDVD from the drive (M9.38, 2026-11-23) |

## 2. Blu-ray and HD

| Status | Item |
|---|---|
| On hold | Blu-ray parental control. It works differently from a DVD's (the player keeps an age, not a level from 1 to 8), and the public documentation found so far does not say how a disc's navigation commands read the player's setting |
| Dependency | Field prediction in H.264 (MBAFF or field pictures) for interlaced sources whose two fields move apart. Conversions to DVD and to Blu-ray's interlaced formats (1080i, SD) keep the fields of an interlaced source apart, scaling and storing each on its own (a bottom-field-first source in that order), and the MPEG-2 encoder predicts and transforms each macroblock as fields (on the pictures where it pays: 0.1 to 0.7 dB better than frames on interlaced material made from 59.94p video, within 0.1 dB where it does not help). The H.264 encoder (craft-video) codes interlaced frames as frames |
| Planned | Measuring the interlaced conversions with real camcorder footage (DV, AVCHD and the like; they are checked with material made by ffmpeg for now) |
| Awaiting merge | Decoding interlaced H.264 (camcorder and recorder 1080i): works through a fork of craft-libs that carries its pull requests [#1992](https://github.com/storytold/craft-libs/pull/1992) and [#2017](https://github.com/storytold/craft-libs/pull/2017) (field pictures and MBAFF), which are not merged yet |
| Awaiting merge | Encoding 1080i as H.264: works through the fork; sent to craft-libs as [issue #2535](https://github.com/storytold/craft-libs/issues/2535) and [pull request #2537](https://github.com/storytold/craft-libs/pull/2537) (a draft stacked on #1992) |
| Needs a decision | Using an ffmpeg the user has installed, optionally, for what DiscCraft's own decoders cannot read (DiscCraft does not use ffmpeg today) |
| Needs hardware | HRD / SEI details that some hardware Blu-ray players may need |
| On hold | 4K (Ultra HD Blu-ray) discs: the video's constraints (HEVC Main 10, 2160p, HDR10) are in a public paper, but the numbers of the disc's structure (the versions of the playlists and clip information, the codes for HEVC and 2160p, the HDR fields) are in no public document, and there is no Ultra HD standard for recordable discs. Converting to HEVC Main 10 (up to 3840 x 2160, BT.709) works (about one frame a second on the processor, about 30 on a Windows GPU), but only in a build made with the `hevc` feature: the published programs leave DiscCraft's own HEVC encoding out until its patents are checked. HDR10 and BT.2020 need the picture path (8 bits and BT.709 today) changed first |
| Out of scope | DTS-HD and Dolby TrueHD as they are |

## 3. DVD

| Status | Item |
|---|---|
| Awaiting decision | Parental levels that differ by country (which level each country gets). DiscCraft now writes the same masks for every country |
| Hardware | Parental blocks and parental levels on home players (a player looks the mask up by its country and level; a player with no level set is taken to play the block's first version) |
| Needs a decision | Multi-angle titles (Encore does not have them) |
| Upstream | Opening VCD-style MPEG-1 system streams in the media reader (DiscCraft works around it) |

## 4. Import and speed

| Status | Item |
|---|---|
| Planned | DV (read since M6.410; the sound in the frames, 12-bit 32 kHz included, since M6.411; DVCPRO50 since M6.415; a camcorder's sound 2 and DVCPRO50's CH3 / CH4 as a second sound stream since M6.418): DVCPRO HD is not read |
| Dependency | DV in an MXF file (a P2 card's) shows no picture: FilmCraft's MXF reader turns DV down before it asks for a decoder, so the DV decoder DiscCraft gives it (used for DV in a `.mov` since M6.413) is not reached. DV in an AVI, a `.mov`, a `.mkv` (M6.416) or a raw `.dv` file is read |
| Dependency | MPEG-4 Part 2 (DivX / Xvid) video is imported but shows no picture. DiscCraft cannot write a decoder of its own for it as it did for DV (M6.410): its specification (ISO/IEC 14496-2) is not published free of charge (it is not on ISO's list of freely available standards; checked on 2026-11-23), and DiscCraft is written from public specifications only, without reading other implementations' source. It will be read when the media reader (craft-libs) gets a decoder |
| Dependency | Other older video codecs: an AVI / MOV / MKV of WMV, H.263, HuffYUV, UT Video, FFV1, QuickTime Animation or PNG frames is imported but shows no picture (the media reader has no decoder for it). HEVC (10-bit too), ProRes, DNxHD, VP9, AV1 and Motion JPEG are read |
| Out of scope | Decoding a source in parts side by side: measured, splitting the work into more threads made the conversion slower (the encoder already uses several cores) |
| Large | The rest of conversion on the GPU: on Windows, HD H.264 (1080p / 720p in one pass) and HEVC (in a build with the `hevc` feature) can be encoded on the hardware encoder (Preferences; off by default; 1080p H.264 at 245 frames a second on NVIDIA, at a quality as good as the processor's or better, with the BT.709 colour description written). The colour conversion runs on every core, which made a build on the hardware about twice as fast. Left: faster decoding and scaling so the hardware's speed shows, interlaced formats (1080i, SD), two passes, macOS and Linux (VideoToolbox / VA-API), and trying the discs on hardware players |
| Awaiting merge | A large MKV's frame index is kept in the media cache, so it is read through once only (before, 15 to 25 seconds for 18 GB at every start). It works through the fork; sent to craft-libs as [issue #2534](https://github.com/storytold/craft-libs/issues/2534) and [pull request #2536](https://github.com/storytold/craft-libs/pull/2536). MP4 files open quickly anyway |
| Awaiting merge | A transport stream's index (`.m2ts` / `.ts` / `.mpg` / `.vob`) is kept in the media cache, so the file is read through once only (before, the whole file was read at every start: 1.8 seconds for 3.9 GB, then under 0.01 seconds the next time, with the file in the OS cache). It works through the fork; sent to craft-libs as [issue #2540](https://github.com/storytold/craft-libs/issues/2540) and [pull request #2542](https://github.com/storytold/craft-libs/pull/2542) (written in a session kept apart from DiscCraft) |

## 5. Timeline, menus and panels

| Status | Item |
|---|---|
| Needs a decision | Threading text from a frame's overset "+" into another frame. Encore's documentation only has the text reflow inside its box when the box is made bigger with a text tool; no threading into another frame was found. Whether to add what Encore does not have |
| Planned | The rest of the automatic routing differences. Routing follows the rules measured in Encore CS6 (since M10.51 also where routes go between a full-width bar and the tiles above it), and of the 696 arrow-key links of the menu of [#13](https://github.com/mOqOm/DiscCraft/issues/13) and the test layouts, all 696 match Encore (M10.60) (counted again with the two pages of the same rectangles paired right; the leftmost button of a page without a previous button, and a button hidden behind the one straight ahead, fixed). Since M10.53 Down from a full-width bar also looks round to the tiles above it as Encore does (1426 of the 1428 links of that measurement match). Since M10.57 a row of tiles above a bar is told as Encore does too, and since M10.58 Down and Right from a bar look round to the same tile as in Encore (all 248 links of the latest measurement, with bars of other sizes, match). Since M10.59 rows and columns also depend on the buttons' order as in Encore (the same layout routes differently), and looking round picks the button beside, ahead or behind as Encore does (219 of the 220 links of the latest measurement match). The rest: a layout where up from a thin strip takes the farther, clockwise button; the column beside a row kept by the buttons' order; and one far layout of the measurement (all of them test layouts) |

## 6. Releases and packages

| Status | Item |
|---|---|
| Needs hardware | Installing with the `.msi`, the Arm64 build, macOS, and the Linux packages (AppImage, Flatpak, deb, rpm, Arch) have not been tried on real machines |
| Needs a decision | Publishing the Flatpak as an OCI image |
| Needs a decision | Signing the programs (Windows SmartScreen and macOS Gatekeeper ask for confirmation the first time) |
| Out of scope | Copy protection (CSS, AACS) |

## 7. Checks on real players

| Status | Item |
|---|---|
| Needs hardware | Discs played on hardware DVD and Blu-ray players |
| Needs hardware | VLC draws Blu-ray highlights a little wider than placed, and draws only the highlighted part of a DVD menu's subpicture; these look like VLC's own behaviour |
