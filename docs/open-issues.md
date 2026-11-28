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
| Hardware | Blu-ray parental control (M6.543): the player keeps an age, not a level from 1 to 8, so each level is given an age (1 all ages, 4 thirteen, 7 eighteen and so on), checked at the start of the disc and of each title. Checked in DiscCraft's preview, not yet on a home player |
| Dependency | Field prediction in H.264 (MBAFF or field pictures) for interlaced sources whose two fields move apart. Conversions to DVD and to Blu-ray's interlaced formats (1080i, SD) keep the fields of an interlaced source apart, scaling and storing each on its own (a bottom-field-first source in that order), and the MPEG-2 encoder predicts and transforms each macroblock as fields (on the pictures where it pays: 0.1 to 0.7 dB better than frames on interlaced material made from 59.94p video, within 0.1 dB where it does not help). The H.264 encoder (craft-video) codes interlaced frames as frames. With the hardware encoder on an NVIDIA card that encodes fields (Blackwell, the RTX 50 series; M12.51), 1080i H.264 is coded as field pictures; elsewhere it stays as frames |
| Planned | Measuring the interlaced conversions with real camcorder footage (DV, AVCHD and the like; they are checked with material made by ffmpeg for now) |
| Awaiting merge | Decoding interlaced H.264 (camcorder and recorder 1080i): works through a fork of craft-libs that carries its pull requests [#1992](https://github.com/storytold/craft-libs/pull/1992) and [#2017](https://github.com/storytold/craft-libs/pull/2017) (field pictures and MBAFF), which are not merged yet |
| Awaiting merge | Encoding 1080i as H.264: works through the fork; sent to craft-libs as [issue #2535](https://github.com/storytold/craft-libs/issues/2535) and [pull request #2537](https://github.com/storytold/craft-libs/pull/2537) (a draft stacked on #1992) |
| Needs a decision | Using an ffmpeg the user has installed, optionally, for what DiscCraft's own decoders cannot read (DiscCraft does not use ffmpeg today) |
| Needs hardware | HRD / SEI details that some hardware Blu-ray players may need |
| On hold | 4K (Ultra HD Blu-ray) discs: the video's constraints (HEVC Main 10, 2160p, HDR10) are in a public paper, but the numbers of the disc's structure (the versions of the playlists and clip information, the codes for HEVC and 2160p, the HDR fields) are in no public document, and there is no Ultra HD standard for recordable discs. Converting to HEVC Main 10 (up to 3840 x 2160, BT.709) works (about one frame a second on the processor, about 30 on a Windows GPU), but only in a build made with the `hevc` feature: the published programs leave DiscCraft's own HEVC encoding out until its patents are checked. HDR10 and BT.2020 need the picture path (8 bits and BT.709 today) changed first |
| Out of scope | DTS-HD and Dolby TrueHD as they are (there is no DTS-HD material to check it with; on a Blu-ray, TrueHD needs AC-3 beside it, which needs a decoder and a published specification) |

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
| Dependency | Other older video codecs: an AVI / MOV / MKV of WMV, HuffYUV, UT Video or QuickTime Animation is imported but shows no picture (the media reader has no decoder for it). PNG frames (M6.561), FFV1 (M6.562, a decoder written from RFC 9043) and H.263 (M6.563, ITU-T H.263's baseline: old mobile phones' 3GP files and the like) in a 3GP / AVI / MOV / MKV are read by DiscCraft itself. H.263 version 2's custom sizes and the annexes' optional modes are not read yet. Hap, uncompressed 10-bit RGB (r210, R10k) and Theora (in an MKV, and `.ogv` with or without sound) are read with craft-libs' video decoders (M6.567). WMV (looked into in M6.564): version 9's video is VC-1, whose standard is free to read (SMPTE ST 421:2013, in SMPTE's public document library), so DiscCraft could decode it itself (a large decoder; VC-1 is also one of the Blu-ray video formats; as of November 2026 its patents have run out in Japan and Europe, but some remain in the United States and Brazil). WMV 7 and 8 have no public specification. A .wmv file itself (ASF) cannot be read: Microsoft's ASF specification lets implementations be distributed only as object code and never under an open-source licence, and the public draft of 1998 describes an earlier version that today's files do not use. HEVC (10-bit too), ProRes, DNxHD, VP9, AV1 and Motion JPEG are read |
| Out of scope | Decoding a source in parts side by side: measured, splitting the work into more threads made the conversion slower (the encoder already uses several cores) |
| Large | The rest of conversion on the GPU: on Windows, HD H.264 (1080p / 720p in one pass) and HEVC (in a build with the `hevc` feature) can be encoded on the hardware encoder (Preferences; off by default; 1080p H.264 at 245 frames a second on NVIDIA, at a quality as good as the processor's or better, with the BT.709 colour description written). The colour conversion runs on every core, which made a build on the hardware about twice as fast, and a source already at the disc's size and colours (1080p BT.709, a Blu-ray's own video for example) goes to the encoder as decoded, without the round trip through RGB, which nearly doubled it again (a Blu-ray's 1080p video at 218 frames a second, against 114). Two passes run on the hardware too: its one pass, then once more at an average corrected by how far the size came out, which lands within about 1 % of the target at a picture as good as the processor's two passes or better, four to six times as fast. 1080i H.264 (1920 or 1440 x 1080) is encoded on the hardware too, through NVIDIA's own encoder interface (NVENC), each frame as its two fields, the top or the bottom one first as the source has them: NVIDIA's encoder through Media Foundation refuses an interlaced stream, and NVIDIA offers field encoding on its Blackwell cards (RTX 50 series) only, so other cards keep it on the processor. A 60-second 1080i Blu-ray took 10.5 seconds against 27.5 on the processor, at the same picture quality. Standard definition stays on the processor (its MPEG-2 is DiscCraft's own encoder). A source of another size in BT.709 (4K or 720p to 1080p, say) is scaled in its own planes too, not through RGB (4K to 1080p at 103 frames a second instead of 49). Sources in other colours (BT.601 standard definition, full range, 10 bits, 4:2:2) have their colours moved to BT.709 in their own planes too, not through RGB (10-bit HEVC and ProRes 4:2:2 at 1080p at 164 to 175 frames a second instead of 80, full range 178 instead of 135, standard definition 180 instead of 137, with better chroma). Left: macOS and Linux (VideoToolbox / VA-API), and trying the discs on hardware players. BT.2020 and HDR, 4:4:4 and sources with alpha still go through RGB |
| Awaiting merge | A large MKV's frame index is kept in the media cache, so it is read through once only (before, 15 to 25 seconds for 18 GB at every start). It works through the fork; sent to craft-libs as [issue #2534](https://github.com/storytold/craft-libs/issues/2534) and [pull request #2536](https://github.com/storytold/craft-libs/pull/2536). MP4 files open quickly anyway |
| Awaiting merge | A transport stream's index (`.m2ts` / `.ts` / `.mpg` / `.vob`) is kept in the media cache, so the file is read through once only (before, the whole file was read at every start: 1.8 seconds for 3.9 GB, then under 0.01 seconds the next time, with the file in the OS cache). It works through the fork; sent to craft-libs as [issue #2540](https://github.com/storytold/craft-libs/issues/2540) and [pull request #2542](https://github.com/storytold/craft-libs/pull/2542) (written in a session kept apart from DiscCraft) |

## 5. Timeline, menus and panels

| Status | Item |
|---|---|
| Needs a decision | Threading text from a frame's overset "+" into another frame. Encore's documentation only has the text reflow inside its box when the box is made bigger with a text tool; no threading into another frame was found. Whether to add what Encore does not have |
| Upstream | A menu of the menu bar opened with Alt+letter (Alt held while the letter is pressed, the usual way) does not take its items' letters; the arrow keys and Enter work. It comes from the menu bar of craft-libs, reported as [issue #4003](https://github.com/storytold/craft-libs/issues/4003) |

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
