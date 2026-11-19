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
| Large | Coding interlaced sources whose two fields move apart as fields. The MPEG-2 encoder can code interlaced pictures, but they did not yet give a better picture, so conversions do not use them |
| Large | The rest of HD MPEG-2 that goes on the disc as it is: field pictures, trimming on GOPs, joining with other clips, HD in program streams |
| Large | Playing an HD motion menu in the preview before the build |
| Upstream | Encoding 1080i as H.264 |
| Upstream | Decoding interlaced H.264 (camcorder and recorder 1080i): its picture cannot be shown yet and it cannot be converted for a DVD (it goes on a Blu-ray as it is, and its sound plays) |
| Needs a decision | Using an ffmpeg the user has installed, optionally, for what DiscCraft's own decoders cannot read (DiscCraft does not use ffmpeg today) |
| Upstream | B pictures in HD MPEG-2 |
| Needs hardware | HRD / SEI details that some hardware Blu-ray players may need |
| Needs a decision | 4K (Ultra HD Blu-ray) |
| Out of scope | DTS-HD and Dolby TrueHD as they are |

## 3. DVD

| Status | Item |
|---|---|
| Planned | Parental masks per country |
| Large | The DVD standard's parental blocks (cells chosen by level). A title can already name what plays in its place when the player's level is too low |
| Needs a decision | Multi-angle titles (Encore does not have them) |
| Upstream | Opening VCD-style MPEG-1 system streams in the media reader (DiscCraft works around it) |

## 4. Import and speed

| Status | Item |
|---|---|
| Planned | Converting HD menus (motion menus) and the titles' sound side by side, as HD titles already are |
| Planned | Muxing HD MPEG-2 from a transport stream without copying it into the cache first (H.264 already is) |
| Planned | Decoding a source in parts side by side (a small gain: the encoder is the slower part) |
| Large | Conversion on the GPU (scaling and colour on the GPU, hardware encoders). Licences come first |
| Upstream | Large MKV / MP4 files are indexed each time they are opened (15 to 25 seconds for 18 GB) |
| Upstream | Keeping a transport stream's index from one run to the next |
| Upstream | Chapter thumbnails of interlaced H.264 transport streams (needs the decoder above) |

## 5. Timeline, menus and panels

| Status | Item |
|---|---|
| Planned | Show in an asset's properties why it is converted (for example, H.264 above level 4.1) |
| Planned | Raise the conversion bit rate when the disc has room to spare |
| Planned | Trimming or removing a video clip also trims or removes its sound beside it (Encore links them) |
| Planned | A title of several clips without audio tracks: the clips' own sound in the build too (the preview plays it already) |
| Large | Formatting single characters inside a paragraph |
| Large | Threading text from a frame's overset "+" into another frame |
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
