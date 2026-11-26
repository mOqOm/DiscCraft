# 23. Troubleshooting

[Contents](README.md) · Previous: [22. Limits and Rules](22-limits.md#22-limits-and-rules) · Next: [24. Glossary](24-glossary.md#24-glossary)

> **In this chapter**
>
> - Learn what to check first when things do not work
> - Learn what to include when reporting an issue

The issues are grouped by type. If you cannot find your problem, also try searching by task in
[1.3 Finding a Task](01-about.md#13-finding-a-task) and checking the [Glossary](24-glossary.md#24-glossary).

## 23.1 Screen and Assets

| Problem | What to do first |
|---|---|
| Cannot find a panel | The **Window** menu lists all panels (you can reopen them here even if closed with ×) |
| Panel layout is messed up | Choose **Window ▸ Workspace ▸ Reset "Name"** |
| Asset is "Offline" | You moved or renamed the asset file. Right-click in the Project panel and choose **Replace Asset…** to specify the new location |
| No sound | Select the track to listen to using the speaker button in the Monitor header. DTS tracks do not play in the Monitor or Preview (but they are included on the disc). On Linux, you need the ALSA development package (such as `libasound2-dev`) |
| Garbled characters | Switch the display language in **Edit ▸ Preferences…**, **Interface**, **Language**. Subtitles support UTF-8 / UTF-16 / Shift_JIS |
| EffectCraft project does not look as expected | Select the composition to use in the **Composition** field of the asset properties |

## 23.2 Menus and Disc Behavior

| Problem | What to do first |
|---|---|
| Want to check disc behavior | Try it with **File ▸ Preview** (Alt+Ctrl+Space) before building ([13.1](13-preview.md#131-project-preview-preview-without-building)) |
| Menu does not appear | Check First Play, button links, and End Actions in the Flowchart ([10.2](10-navigation.md#102-flowchart)) |
| Buttons do not work | Check if highlights overlap (red border), and view arrow destinations with **View ▸ Show Button Routing** |
| Cannot reach hidden buttons | Check if a route extends from a reachable button: [17.4 Hidden Buttons and Easter Eggs](17-advanced.md#174-hidden-buttons-and-easter-eggs) |

## 23.3 Build and Disc

| Problem | What to do first |
|---|---|
| Build stops with "Problems found" | Open **File ▸ Check Project…** and review the **Name** and **Problem** columns ([Chapter 12](12-check.md#12-checking-the-project)). If you cannot fix it, choose **Ignore and Continue** in the dialog |
| Content does not fit on disc | Check content size using the usage bar in the Build panel (**Refresh Usage**). Solutions are: reduce content, lower bitrate, or use a larger disc ([17.9](17-advanced.md#179-monitoring-disc-size-while-working)) |
| Build takes too long | Converting incompatible assets takes time. You can finish this before building with **Transcode Now** ([5.3](05-assets.md#53-about-assets)). The number of simultaneous conversions and Windows hardware encoders are set in Preferences under [18.7](18-preferences.md#187-memory) |
| Cannot write to disc | If no recorder appears, check if a writable drive is connected and search again with **Refresh**. Write dual-layer DVDs to blank DVD+R DL discs. Writing is not yet supported on macOS and Linux; create an ISO image and write it with other software ([14.7](14-build.md#147-writing-to-a-disc)) |
| Written disc does not play in player | First check if it works in Project Preview. Check the player manual to see if it supports that disc type (DVD-R, DVD+R, BD-R, etc.). Opening the ISO image with a PC player helps determine if the issue is with the disc or the image |

## 23.4 Reporting and New Versions

| Problem | What to do first |
|---|---|
| Want to report an issue | Use **Copy** in **Help ▸ System Compatibility Report…** for environment info, and **Help ▸ Reveal Log Files…** for the operation log since this launch, then attach them to **Help ▸ Report an Issue…**. If DiscCraft crashes unexpectedly, the log from that time remains in the log folder |
| Want to investigate playback compatibility in detail | Open the written folder or image in a PC player (VLC and the like) to see where it stops |
| Want to know if a new version is available | Check this version with **Help ▸ Check for Updates…** and open the Releases on GitHub (DiscCraft does not communicate on its own) |
