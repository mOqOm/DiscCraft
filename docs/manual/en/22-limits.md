# 22. Limits and Rules

[Contents](README.md) · Previous: [21. Using DiscCraft from AI Agents (MCP)](21-mcp.md#21-using-disccraft-from-ai-agents-mcp) · Next: [23. Troubleshooting](23-troubleshooting.md#23-troubleshooting)

> **In this chapter**
>
> - Learn the limits of DVD and Blu-ray (number of titles and buttons, sizes)

| Item | Limit |
|---|---|
| Buttons | 36 per menu |
| Buttons on a 16:9 DVD menu (including highlights for 4:3 TVs) | 18 |
| Menu duration | 254 s |
| Video clips in a timeline | 99 |
| Chapters | 99 per title (DVD) |
| DVD titles | 99 |
| Audio tracks | 8 |
| Subtitle tracks | 32 |
| Chapter index | 18 buttons per page (6 buttons per page for multiple pages) |
| Slide duration | Default 6 s. Transition 0.1–10 s (default 2 s) |
| Subtitle trim | Minimum 0.4 s (NTSC 13 / PAL 11 frames) |
| Subtitle characters | Up to 10 lines at standard NTSC size within the title-safe area |
| Subtitle gap | Blu-ray (HD / PAL) requires 5 frames or more (no limit for DVD) |
| DVD bitrate | 9.8 Mbps total for video, audio, and subtitles |
| Capacity | DVD5 4.7 GB, DVD9 8.5 GB, BD25 / BD50 / BD100 / BD128, custom sizes |
| Drop | 500 files per operation |
| Recent Projects | 10 items |
| Workspace | 32 (names up to 64 characters) |

Other rules:

- Button highlights do not overlap on the disc (they can overlap in the editing screen).
- A timeline must have video as its first track.
- MPEG-2 that goes directly onto a disc cuts at GOP boundaries (trims and chapters snap to the nearest GOP boundary).
  Converted footage cuts at frame positions.
- 16:9 menus on SD use the same 720×480 frame, stretched horizontally for display (HD Blu-ray uses a 1920×1080 frame).
  The aspect ratio of DVD menus is common across the entire disc.
- Audio converts to Dolby Digital (AC-3) at 192 kbps. AC-3 and WAV assets can be used as-is. DTS (`.dts`) can also be
  used as-is. Dolby Digital Plus (E-AC-3: `.eac3`, `.ec3`, or an `.ac3` holding E-AC-3) is converted to Dolby Digital.
- MPEG-2 video compatible with DVD (widths of 720 / 704 / 352) is not transcoded. Clips within a single title must have
  the same width.
- MPEG-1 video (VCD `.mpg`, `.m1v`) can be placed directly onto a DVD in 4:3 if it is 352x240 (NTSC) or 352x288 (PAL)
  and the bitrate indicated by the stream is up to 1.856 Mbps. For files marked as variable bitrate, the entire file is
  measured; if no single second exceeds 1.856 Mbps, it is used as-is; otherwise, it is transcoded. MPEG audio is
  converted to Dolby Digital. Do not mix MPEG-1 and MPEG-2 clips in a single title. On Blu-ray, MPEG-1 is always
  transcoded.
