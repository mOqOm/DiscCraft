# DVD-Video フォーマットメモ（M2 の実装用）

DiscCraft が書き出す `VIDEO_TS` の構造。クリーンルームの方針（AGENTS.md ルール 3）に従い、公開資料の記述だけから書いている。
他の実装のソースは参照していない。確証の薄い値には「要確認」と書き、外部オラクル（ffprobe、VLC、実機）で確かめる。

参照した公開資料:
- ISO/IEC 13818-1（MPEG-2 Systems：パック、PES、SCR / PTS / DTS、システムヘッダ）
- ISO/IEC 13818-2（MPEG-2 Video：シーケンスヘッダ、GOP、ピクチャヘッダ、ピクチャコーディング拡張）
- ATSC A/52（AC-3 の syncinfo / bsi）
- DVD-Video の構造について公開されている解説資料（IFO、PGC、NAV パックのフィールド表）
- MPUCoder「DVD-Video information」（http://www.mpucoder.com/DVD/ ：vmi.html、pgc.html、pci_pkt.html、ifo_vmg.html。2026-11-08 参照）

All multi-byte integers are **big-endian**. A sector is **2048 bytes**. "Sector pointer" = sector index from the start of the file.
"Relative sector" in VOBS context = sector index from the first sector of the title VOBS (VTS_xx_1.VOB is sector 0, files concatenated).

## 1. Files written for one title set without menus (M2)

```
VIDEO_TS/VIDEO_TS.IFO   VMGI
VIDEO_TS/VIDEO_TS.BUP   byte-identical copy of VIDEO_TS.IFO
VIDEO_TS/VTS_01_0.IFO   VTSI of title set 1
VIDEO_TS/VTS_01_1.VOB   title VOBS, split into _1, _2, ... at 1 GiB (524,288 sectors) boundaries, max 9 files
VIDEO_TS/VTS_01_0.BUP   copy of VTS_01_0.IFO
```
One timeline = one title set (VTS) with one title, one PGC, one VOB (VOB id 1), one cell per chapter (cell id 1..n), one program per cell.
Sector addresses that refer to other files (TT_SRPT VTS start) assume the files are laid out contiguously on disc in the order
VIDEO_TS.IFO, (VIDEO_TS.VOB), VIDEO_TS.BUP, VTS_01_0.IFO, (VTS_01_0.VOB), VTS_01_1.VOB.., VTS_01_0.BUP, VTS_02_0.IFO, ...
and are counted from the first sector of VIDEO_TS.IFO.

## 2. Common encodings

**BCD time (4 bytes)**: hours BCD, minutes BCD, seconds BCD, frames byte = `rate << 6 | frames BCD (6 bits: tens in bits 5-4)`,
rate `0b11` = 29.97 fps (NTSC), `0b01` = 25 fps (PAL). Frames are non-drop frame counts within the second (floor).

**Video attributes (2 bytes)**: byte0 = `coding << 6 | standard << 4 | aspect << 2 | flags`; coding 1 = MPEG-2; standard 0 = NTSC, 1 = PAL;
aspect 0 = 4:3, 3 = 16:9; flags 0 for 4:3. byte1 = `resolution << 3` (0 = 720x480/576, 1 = 704, 2 = 352x480/576, 3 = 352x240/288).
M2 supports MPEG-2 720x480 / 720x576 4:3 only → NTSC `40 00`, PAL `50 00`.
M7: 16:9 titles set aspect 3 and "automatic pan/scan disallowed" (bit 1; letterbox allowed, bit 0 = 0) → NTSC `4E 00`, PAL `5E 00`.
Menus stay 4:3. Subpicture stream control for 16:9 titles: byte 0 = `0x80 | stream` (4:3), byte 1 = wide, byte 2 = letterbox, byte 3 = pan&scan — DiscCraft writes the same stream number in all four.

**Audio attributes (8 bytes)**: byte0 = `coding << 5 | lang_type << 2` (coding 0 = AC-3, 4 = LPCM; lang_type 1 = language code present);
byte1 = `quant << 6 | rate << 4 | (channels - 1)` (quant 0 = 16-bit for LPCM / 0 for AC-3; rate 0 = 48 kHz); bytes 2-3 = ISO 639-1 code
in ASCII (e.g. `ja`), zero if none; byte 4 reserved; byte 5 code extension (0); bytes 6-7 zero.

**VM command (8 bytes)** used in M2: `JumpTT n` = `30 02 00 00 00 nn 00 00`; `Exit` = `30 01 00 00 00 00 00 00`.

## 3. VMGI — VIDEO_TS.IFO

Layout: sector 0 = VMGI_MAT (0x000..0x3FF) + FP_PGC at byte 0x400; then each table starts on its own sector: TT_SRPT, VTS_ATRT.
File size = whole sectors (zero padded).

VMGI_MAT:
| off | size | value |
|---|---|---|
| 0x000 | 12 | `DVDVIDEO-VMG` |
| 0x00C | 4 | last sector of the VMG set (IFO + VMGM VOBS + BUP sectors − 1) |
| 0x01C | 4 | last sector of VMGI (IFO sectors − 1) |
| 0x020 | 2 | version `0x0011` |
| 0x022 | 4 | VMG category: byte 0x023 = region mask, bit i set = region i+1 **prohibited**; 0x00 = all regions |
| 0x026 | 2 | number of volumes = 1 |
| 0x028 | 2 | volume number = 1 |
| 0x02A | 1 | side = 1 |
| 0x03E | 2 | number of title sets |
| 0x040 | 32 | provider id (ASCII, zero padded) `DiscCraft` |
| 0x080 | 4 | end byte address of VMGI_MAT **including the First Play PGC** = `0x400 + FP_PGC size − 1`（外部オラクルが `first_play_pgc < last byte` を検査する） |
| 0x084 | 4 | byte address of FP_PGC = `0x400` |
| 0x0C0 | 4 | start sector of VMGM_VOBS (0 = none) |
| 0x0C4 | 4 | sector pointer to TT_SRPT |
| 0x0C8 | 4 | sector pointer to VMGM_PGCI_UT (0 = none) |
| 0x0CC | 4 | PTL_MAIT (0) |
| 0x0D0 | 4 | sector pointer to VTS_ATRT |
| 0x0D4 | 4 | TXTDT_MG (0) |
| 0x0D8 | 4 | VMGM_C_ADT (0 = none) |
| 0x0DC | 4 | VMGM_VOBU_ADMAP (0 = none) |
| 0x100 | 2 | VMGM video attributes (M2: same as titles) |
| 0x102 | 2 | number of VMGM audio streams (0) |
| 0x154 | 2 | number of VMGM subpicture streams (0) |
All other bytes zero.

**FP_PGC** (first play): a PGC (§5) with 0 programs, 0 cells, and one pre command `JumpTT 1` (M2: no menus).

**TT_SRPT**: `u16 n_titles, u16 0, u32 end address (= 8 + 12·n − 1)`, then per title (12 bytes):
| off | size | value |
|---|---|---|
| 0 | 1 | title type: `0x3C` (要確認：コマンドの有無を示すフラグ。単純なタイトルで一般に見られる値) |
| 1 | 1 | number of angles = 1 |
| 2 | 2 | number of chapters (PTTs) |
| 4 | 2 | parental mask 0 |
| 6 | 1 | VTS number (1-based) |
| 7 | 1 | title number within the VTS (VTS_TTN): 1 for a timeline or slideshow; a chapter playlist is title 2, 3, … of its master timeline's VTS (M6.109) |
| 8 | 4 | start sector of the VTS (its VTS_xx_0.IFO), counted from VIDEO_TS.IFO (§1) |

**VTS_ATRT**: `u16 n_vts, u16 0, u32 end address`, then `n` u32 offsets (relative to VTS_ATRT start) to each VTS_ATR.
VTS_ATR (0x308 bytes): `u32 end address = 0x307`, `u32 VTS category` (copy of VTSI_MAT 0x022), then 0x300 bytes copied from
VTSI_MAT 0x100..0x3FF. End address of VTS_ATRT = 8 + 4·n + 0x308·n − 1.

## 4. VTSI — VTS_xx_0.IFO

Layout: sector 0 = VTSI_MAT; then each on its own sector: VTS_PTT_SRPT, VTS_PGCITI, VTS_TMAPTI, VTS_C_ADT, VTS_VOBU_ADMAP.

VTSI_MAT:
| off | size | value |
|---|---|---|
| 0x000 | 12 | `DVDVIDEO-VTS` |
| 0x00C | 4 | last sector of the title set (IFO + menu VOBS + title VOBS + BUP − 1) |
| 0x01C | 4 | last sector of VTSI (IFO sectors − 1) |
| 0x020 | 2 | version `0x0011` |
| 0x022 | 4 | VTS category 0 |
| 0x080 | 4 | end byte address of VTSI_MAT = `0x3FF` |
| 0x0C0 | 4 | start sector of VTSM_VOBS (0 = none) |
| 0x0C4 | 4 | start sector of VTST_VOBS (= IFO sectors when there is no menu VOBS) |
| 0x0C8 | 4 | sector pointer to VTS_PTT_SRPT |
| 0x0CC | 4 | sector pointer to VTS_PGCITI |
| 0x0D0 | 4 | VTSM_PGCI_UT (0) |
| 0x0D4 | 4 | sector pointer to VTS_TMAPTI |
| 0x0D8 | 4 | VTSM_C_ADT (0) |
| 0x0DC | 4 | VTSM_VOBU_ADMAP (0) |
| 0x0E0 | 4 | sector pointer to VTS_C_ADT |
| 0x0E4 | 4 | sector pointer to VTS_VOBU_ADMAP |
| 0x100 | 2 | VTSM video attributes (same as title) |
| 0x200 | 2 | VTS video attributes |
| 0x202 | 2 | number of audio streams (0..8) |
| 0x204 | 8·8 | audio attributes |
| 0x254 | 2 | number of subpicture streams (0) |

**VTS_PTT_SRPT**: `u16 n_titles, u16 0, u32 end address`, `n` u32 offsets (relative to table start) to each title's PTT list;
each PTT entry is `u16 PGCN, u16 PGN` (chapter k of VTS title t → PGC t, program k). End address = 8 + 4·n + 4·Σchapters − 1.

**VTS_PGCITI**: `u16 n_pgcs, u16 0, u32 end address`, then per PGC 8 bytes: `u32 category` (byte 0 = `0x80 | VTS_TTN` for the
entry PGC of a title, other bytes 0), `u32 offset` to the PGC (relative to VTS_PGCITI start). PGCs follow. PGC n is the entry
PGC of VTS title n: PGC 1 plays the timeline's cells in order; a chapter playlist (M6.109) is a further PGC that lists the same
cells (VOB id / cell id / sectors) in its own order, so it adds no VOBS data.

**VTS_TMAPTI**: `u16 n_pgcs, u16 0, u32 end address`, `n` u32 offsets to VTS_TMAP (one per PGC, in PGC order, packed in that
order). VTS_TMAP: `u8 time unit (seconds, ≥1)`, `u8 0`, `u16 n_entries`, then `n` u32 = relative sector of the VOBU that contains
time `(i+1)·unit` of that PGC's own running time; bit 31 = discontinuity (0). Unit = max(1, ceil(duration / 2048 s));
n = floor(duration / unit).

**VTS_C_ADT**: `u16 n_vob_ids, u16 0, u32 end address (= 8 + 12·n_cells − 1)`, then per distinct cell of all the PGCs (sorted by
VOB id, cell id; a cell shared by several PGCs is listed once) 12 bytes: `u16 VOB id, u8 cell id, u8 0, u32 first sector,
u32 last sector` (relative sectors).

**VTS_VOBU_ADMAP**: `u32 end address (= 4 + 4·n − 1)`, then `n` u32 = relative start sector of every VOBU in VTST_VOBS.

## 5. PGC

| off | size | value |
|---|---|---|
| 0x00 | 2 | 0 |
| 0x02 | 1 | number of programs |
| 0x03 | 1 | number of cells |
| 0x04 | 4 | playback time (BCD) |
| 0x08 | 4 | prohibited user operations (UOPs), big-endian, UOP0 = bit 0: 0 time play/search, 1 chapter play/search, 2 title play, 3 stop, 4 go up, 5 time/chapter search, 6 top/prev PG, 7 next PG, 8 forward scan, 9 backward scan, 10 menu call title, 11 menu call root, 12 subpicture menu, 13 audio menu, 14 angle menu, 15 chapter menu, 16 resume, 17 button select/activate, 18 still off, 19 pause on, 20 audio change, 21 subpicture change, 22 angle change, 23 karaoke change, 24 video mode change; bits 25..31 reserved. Title PGCs and VMGM menu PGCs carry the timeline's / menu's prohibited operations (M6.61); other PGCs 0 |
| 0x0C | 2·8 | audio stream control: stream i present → `0x80 \| i` in byte 0, byte 1 = 0 |
| 0x1C | 4·32 | subpicture stream control (0 = none) |
| 0x9C | 2 | next PGCN 0 |
| 0x9E | 2 | prev PGCN 0 |
| 0xA0 | 2 | go-up PGCN 0 |
| 0xA2 | 1 | PG playback mode: 0 = sequential; `0x80 \| (n − 1)` = random (n programs, repeating); `0xC0 \| (n − 1)` = shuffle (n programs, each once). Chapter playlists use it for Encore's 再生の種類 (M6.109); n is at most 64 |
| 0xA3 | 1 | PGC still time (0; 255 = infinite) |
| 0xA4 | 4·16 | palette `00 Y Cr Cb` (M2: 16 × `00 10 80 80`) |
| 0xE4 | 2 | offset of command table (0 = none) |
| 0xE6 | 2 | offset of program map (0 if no programs) |
| 0xE8 | 2 | offset of cell playback table (0 if no cells) |
| 0xEA | 2 | offset of cell position table (0 if no cells) |
| 0xEC | | data: command table, program map, cell playback, cell position (in that order; offsets relative to PGC start) |

Command table: `u16 n_pre, u16 n_post, u16 n_cell, u16 end address (= 8 + 8·total − 1)`, then the commands.
Program map: one byte per program = entry cell number (1-based); pad to an even length.
Cell playback (24 bytes per cell): `u8 category` (`0x02` = STC discontinuity for the first cell of a VOB and for every cell of a
chapter playlist PGC that does not directly follow the previous cell in the VOB, else `0x00`), `u8 0`,
`u8 still 0`, `u8 cell command 0`, `BCD playback time`, `u32 first VOBU start sector`, `u32 first ILVU end sector 0`,
`u32 last VOBU start sector`, `u32 last sector of the cell` (relative sectors).
Cell position (4 bytes per cell): `u16 VOB id, u8 0, u8 cell id`.
Title PGC in M2: post command = `Exit` (end action Stop); later milestones replace it with jumps.

## 6. VOBS (MPEG-2 program stream, DVD constraints)

- Every pack is exactly 2048 bytes: a 14-byte pack header followed by one PES packet (or a NAV pack's two packets), padded with a
  padding packet (`00 00 01 BE`, length, `FF..`) when the payload is short. A padding packet needs ≥ 6 bytes; if fewer bytes remain,
  enlarge the PES header with stuffing bytes (`FF`, up to 32... keep it simple: stuff the PES header) instead.
- Pack header: `00 00 01 BA`, 6 bytes MPEG-2 SCR (`01 base[32..30] 1 base[29..15] 1 base[14..0] 1 ext(9) 1`), mux rate 25200
  (`01 89 C3`), `F8` (no stuffing). SCR base is 90 kHz, ext 27 MHz remainder; SCR of pack i = i · 2048 / (25200 · 50) s.
  Packs whose data is due earlier than the SCR would allow are a mux error (report it, never drop data).
- VOBU = NAV pack + the packs of one GOP (0.4..1.0 s). Each VOBU starts with a NAV pack, then a video pack beginning with the sequence
  header + GOP header. Closed GOPs are expected for passthrough input; the first GOP of every cell must be closed.
- Video PES: stream id `E0`, `0x81`, PTS (+DTS when DTS ≠ PTS) on a packet in which a picture starts (the first picture starting in it).
- AC-3: private stream 1 (`BD`), PTS, then sub stream id `0x80 + n`, `u8 number of frame headers starting in this packet`,
  `u16 first access unit pointer` (offset of the first frame start counted from the byte after this field, plus 1).
- LPCM: `BD`, sub stream `0xA0 + n`, `u8 frame count`, `u16 first access unit pointer`, `u8 0` (emphasis/mute/frame no.),
  `u8 quant << 6 | rate << 4 | (ch − 1)` (16-bit, 48 kHz → `0x01` for stereo), `u8 0x80` (no DRC). Samples big-endian. A LPCM frame is 1/600 s.
- Timing: first video DTS = 0.3 s (27000 @ 90 kHz) after SCR 0; audio PTS aligned so that audio and video start presentation together.
- The last video packet of the VOBS ends with a sequence end code `00 00 01 B7`.

### 6.1 NAV pack (exactly 2048 bytes)

`pack header (14)` + `system header (24)` + `PCI packet (6 + 980)` + `DSI packet (6 + 1018)`.
System header bytes (constant): `00 00 01 BB 00 12 80 C4 E1 04 E1 FF B9 E0 E8 B8 E0 08 BD E0 3A BF E0 02`.
PCI packet: `00 00 01 BF 03 D4 00` + 979 bytes PCI. DSI packet: `00 00 01 BF 03 FA 01` + 1017 bytes DSI.
Offsets below are within the PCI / DSI data (after the sub stream byte).

PCI (M2: only PCI_GI, the rest zero — no buttons):
| off | size | field |
|---|---|---|
| 0x00 | 4 | NV_PCK_LBN: relative sector of this NAV pack |
| 0x04 | 2 | VOBU category 0 |
| 0x08 | 4 | VOBU UOP control 0 |
| 0x0C | 4 | VOBU_S_PTM: PTS (90 kHz) of the first video frame presented in this VOBU |
| 0x10 | 4 | VOBU_E_PTM: VOBU_S_PTM of the next VOBU (end of presentation) |
| 0x14 | 4 | VOBU_SE_E_PTM: end PTM if this VOBU ends with a sequence end code, else 0 |
| 0x18 | 4 | C_ELTM: elapsed time in the cell at VOBU start (BCD) |

DSI:
| off | size | field |
|---|---|---|
| 0x00 | 4 | NV_PCK_SCR: SCR base (90 kHz, low 32 bits) of this NAV pack |
| 0x04 | 4 | NV_PCK_LBN |
| 0x08 | 4 | VOBU_EA: offset (sectors) from the NAV pack to the last pack of this VOBU |
| 0x0C | 4 | VOBU_1STREF_EA: offset to the last pack containing data of the first reference (I) picture |
| 0x10 | 4 | VOBU_2NDREF_EA: same for the 2nd reference picture (I/P) in decode order, 0 if none |
| 0x14 | 4 | VOBU_3RDREF_EA: same for the 3rd, 0 if none |
| 0x18 | 2 | VOBU_VOB_IDN (VOB id) |
| 0x1B | 1 | VOBU_C_IDN (cell id) |
| 0x1C | 4 | C_ELTM (BCD), same as PCI |
| 0x2C | 4 | VOB_V_S_PTM: first video PTS of the VOB |
| 0x30 | 4 | VOB_V_E_PTM: end video PTS of the VOB |
| 0xEA | 4·42 | VOBU_SRI (search information), see below |
| 0x192 | 2·8 | A_SYNCA: per audio stream, offset (sectors) from the NAV pack to the first pack of that stream in the VOBU; `0x3FFF` if none (要確認) |
| 0x1A2 | 4·32 | SP_SYNCA: 0 (no subpictures) |

VOBU_SRI, 42 u32 entries in this order: `[0]` next VOBU with video, `[1..=19]` forward 240, 120, 60, 20, 15, 14, ..., 2, 1,
`[20]` next VOBU, `[21]` previous VOBU, `[22..=40]` backward 1, 2, ..., 15, 20, 60, 120, 240, `[41]` previous VOBU with video.
Step n means the VOBU that starts n × 0.5 s later / earlier. Each entry = `0x80000000 | offset` (offset in sectors from this NAV pack,
always positive) when the target exists inside the same cell, else `0x3FFFFFFF`. (要確認：bit 30 の意味。M2 では 0)

## 7. Validation plan

1. `ifo` reads back what it wrote (round-trip tests) and the reader rejects truncated / inconsistent input without panicking.
2. ffprobe reads every VOB with `-v error` and reports no errors; the video and AC-3 / LPCM streams are detected with the right counts.
3. VLC opens the `VIDEO_TS` folder as a DVD, plays, and chapter next/previous works (manual, recorded in docs/hardware-compat.md).

## 8. Menus and navigation (M3)

Source for the bit layouts in this section: MPUCoder's VM instruction table, PGC, PCI and VMG pages (see the list at the top).

### 8.1 VM instructions used by DiscCraft

Bit positions count from the MSB of byte 0. Byte 0 = `type << 5 | direct << 4 | set_op`; byte 1 = `cmp_imm << 7 | cmp_op << 4 | cmd`.
Compare ops: 1 `&`, 2 `==`, 3 `!=`, 4 `>=`, 5 `>`, 6 `<=`, 7 `<`. Registers: 0..=15 GPRM, 0x80..=0x97 SPRM. Set ops: 1 mov, 2 swp, 3 add,
4 sub, 5 mul, 6 div, 7 mod, 8 rnd, 9 and, 10 or, 11 xor.

| mnemonic | bytes |
|---|---|
| `Exit` | `30 01 00 00 00 00 00 00` |
| `JumpTT t` | `30 02 00 00 00 tt 00 00` |
| `JumpSS VMGM pgcn p` (FP / VMGM / VTSM domains only) | `30 06 pp pp 00 C0 00 00` |
| `CallSS VMGM pgcn p, rsm_cell r` (title domain) | `30 08 pp pp rr C0 00 00` |
| `LinkPGCN p` | `20 04 00 00 00 00 pp pp` |
| `LinkPGN g [, button b]` | `20 06 00 00 00 00 (b << 2) gg` |
| `if (GPRM r == v) LinkPGN g` | `20 A6 00 rr vv vv 00 gg` |
| `if (GPRM r == v) LinkPGCN p` | `20 A4 00 rr vv vv pp pp` |
| `if (GPRM r == v) Goto line l` | `00 A1 00 rr vv vv 00 ll` |
| `GPRM r = v` (mov immediate) | `71 00 00 0r vv vv 00 00` |
| `GPRM d = GPRM s` (mov register) | `61 00 00 0d 00 ss 00 00` |
| `SetHL_BTNN b` (immediate) | `56 00 00 00 (b << 2) 00 00 00` (value = b · 1024) |
| `SetSTN` (set system: audio / subpicture / angle streams, immediate) | `51 01 00 aa ss gg 00 00`: aa = `0x80 \| audio stream (0..7)`, ss = `0x80 \| display << 6 \| subpicture stream (0..31)`, gg = `0x80 \| angle`; a clear top bit leaves that register alone. DiscCraft writes it first in the first-play PGC for the disc's Set Audio / Set Subtitle (M6.72) |
| `if (GPRM a <op> GPRM b) CallSS VMGM pgcn p` (register compare, jumps) | `30 (op << 4 \| 8) pp pp 00 C0 aa bb` |
| `if (GPRM a <op> GPRM b) JumpTT t` | `30 (op << 4 \| 2) 00 00 00 tt aa bb` |

Domain rule used by the compiler: `JumpSS` only from FP and menu PGCs; a title PGC returns to a menu with `CallSS VMGM pgcn`.

### 8.2 Register plan (nav compiler)

| register | use |
|---|---|
| GPRM0 | requested chapter for the next `JumpTT` (0 = start of title) |
| GPRM1 | scratch copy of GPRM0 inside a title's pre commands |
| GPRM2 | VMGM PGCN of the last menu shown (for "last menu") |

- **Menu PGC** pre commands: `GPRM2 = own pgcn`.
- **Chapter entry**: a button / first play cannot set a register and jump in one command, so for every target (title t, chapter c > 1) the
  VMGM gets a hidden *dispatch PGC* (no cells) with pre commands `GPRM0 = c; JumpTT t`; the button does `LinkPGCN dispatch`.
  Chapter 1 targets use `JumpTT t` directly.
- **Title PGC** pre commands: `GPRM1 = GPRM0; GPRM0 = 0; if (GPRM1 == c) LinkPGN c` for c = 2..=chapters.
- **Title PGC** cells: still time 0, except a manual-advance slideshow (M6.90), whose cells (one per slide) carry still time `0xFF` so each slide holds until Next / Enter.
- **Last menu**: one hidden *last-menu PGC* with pre commands `if (GPRM2 == p) LinkPGCN p` for every menu p, then `LinkPGCN first_menu`.
- **End action** (title post commands): Stop → `Exit`; Menu m → `CallSS VMGM pgcn m, 0`; LastMenu → `CallSS VMGM pgcn last_menu_pgc, 0`;
  Timeline t chapter c → `GPRM0 = c; JumpTT t` (c = 1 → just `JumpTT t`).
- **First play**: Menu m → `JumpSS VMGM pgcn m`; Timeline → as end action; NotSet → first menu if any, else `JumpTT 1`.

### 8.3 VMGM structures

- `VIDEO_TS.VOB` holds the VMGM VOBS; VMGI_MAT 0x0C0 = its start sector (= VMGI sectors); 0x00C last sector of the VMG set includes it.
- VMGI_MAT 0x154 = 1 subpicture stream, 0x156 = 6 bytes attributes (all zero: 2-bit RLE, no language).
- VMGM video attribute (VMGI_MAT 0x100) and every menu still / motion sequence header: 16:9 when the project's first menu is 16:9, else 4:3 (M6.75; one attribute covers every VMGM menu, so DiscCraft warns when menus mix).
- VMGI tables added after VTS_ATRT, each on its own sector: VMGM_PGCI_UT (0x0C8), VMGM_C_ADT (0x0D8), VMGM_VOBU_ADMAP (0x0DC);
  same layouts as VTS_C_ADT / VTS_VOBU_ADMAP.
- **VMGM_PGCI_UT**: `u16 n_lu, u16 0, u32 end address`; per LU 8 bytes: `u16 ISO 639 code` (DiscCraft: `en`), `u8 0`,
  `u8 menu existence` (`0x80` = title menu exists), `u32 offset to the LU`. LU: `u16 n_pgcs, u16 0, u32 end address`; per PGC 8 bytes:
  `u32 category` (byte 0: `0x80 | menu type` for an entry PGC, menu type 2 = title menu; 0 for others), `u32 offset to the PGC (from the LU)`;
  then the PGCs. Offsets are relative to the table / LU start.
- **Menu PGC**: 1 program, 1 cell; cell still time `0xFF` (infinite); VOB id = menu index + 1, cell id 1; audio control empty;
  subpicture control entry 0 = `0x80 00 00 00` (stream 0 available); palette = the menu's 16-colour CLUT `00 Y Cr Cb`.

### 8.4 Menu VOB and highlights

A menu VOB is one VOBU: NAV pack, the still frame (one I picture + sequence end), and one subpicture unit on private stream 1 sub
stream `0x20` (forced display, PTS = video PTS). The subpicture holds the highlight shapes (pixel values 1..3 inside each button's rectangle,
0 elsewhere) with all contrasts 0 so nothing shows until a button is selected.

PCI HLI (offsets within the PCI data):
| off | size | field |
|---|---|---|
| 0x60 | 2 | highlight status: `0x0001` (all new highlight information in this VOBU) |
| 0x62 | 4 | HLI start PTM = VOBU_S_PTM |
| 0x66 | 4 | HLI end PTM `0xFFFFFFFF` (要確認：静止メニューでは終了なし) |
| 0x6A | 4 | button selection end PTM `0xFFFFFFFF` (要確認) |
| 0x6E | 2 | button mode: byte 0 = `n_groups << 4 | group-1 type` (1 group, type 1 = normal) → `0x11`, byte 1 = 0 |
| 0x70 | 1 | starting button number (BTN_OFN): 1, or the menu's button number offset + 1 (M6.100, Encore's menu Offset) |
| 0x71 | 1 | number of buttons |
| 0x72 | 1 | number of numerically selectable buttons (= number of buttons) |
| 0x74 | 1 | forced select button (default button number, 0 = none) |
| 0x75 | 1 | forced action button 0 |
| 0x76 | 3 × 8 | SL_COLI 1..3: selection colour (nibbles e2 e1 p b), selection contrast, action colour, action contrast (2 bytes each). M6.97: entry n is highlight group n of the menu's colour set — selection pattern colour = palette 1 / 4 / 6 (the group's selected colour), action = palette 2 / 5 / 7 (activated); emphasis = palette 3 (text colour); contrasts `0x00 0xF0` |
| 0x8E | 36 × 18 | BTN_IT |

BTN_IT entry (18 bytes): byte 0 = `coln << 6 | x1 >> 4`, byte 1 = `(x1 & 0xF) << 4 | x2 >> 8`, byte 2 = `x2 & 0xFF`,
byte 3 = `auto_action << 6 | y1 >> 4`, byte 4 = `(y1 & 0xF) << 4 | y2 >> 8`, byte 5 = `y2 & 0xFF`, bytes 6..=9 = adjacent button numbers
up / down / left / right (6 bits), bytes 10..=17 = the button's VM command. `coln` 1..=3 selects an SL_COLI entry. Unused entries are zero.
