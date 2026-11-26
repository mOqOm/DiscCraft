# 20. Command Line and Scripts

[Contents](README.md) · Previous: [19. Keyboard Shortcuts](19-shortcuts.md#19-keyboard-shortcuts) · Next: [21. Using DiscCraft from AI Agents (MCP)](21-mcp.md#21-using-disccraft-from-ai-agents-mcp)

> **In this chapter**
>
> - Create and build projects from the command line

You can perform the same operations as in the GUI using `disccraft-cli` (building, inspecting projects, running
scripts).

```
cargo run -p disccraft-cli -- commands [--json]
cargo run -p disccraft-cli -- exec <project.dcproj> <command> [params-json]
cargo run -p disccraft-cli -- run <project.dcproj> <script.jsonl>
cargo run -p disccraft-cli -- inspect <project.dcproj> [--json]
cargo run -p disccraft-cli -- build <project.dcproj> --out <dir>
cargo run -p disccraft-cli -- build <project.dcproj> --iso <file> [--cache <dir>] [--audio-language ja]
cargo run -p disccraft-cli -- build <project.dcproj> --ddp <dir>
cargo run -p disccraft-cli -- bd-chapters <D: | disc folder | .mpls> [--playlist 1 | --all] [--json | --ogm]
cargo run -p disccraft-cli -- disc-info [--recorder E:]
```

- `commands` lists the engine commands (ID, display name, menu location, shortcut, parameters). Use `--json` for JSON
  output.
- `exec` runs a single command and outputs the result as JSON. If the project is unsaved, it saves it.
- `run` executes a JSON Lines script. Each line is one command in the format `{"command": "...", "params": {...}}`.
  Lines starting with `#` or `//` are ignored. A script may be up to 16 MB.
- `inspect` provides a project summary (use `--json` for the JSON of `project.inspect`).
- `build` creates a folder with `--out`, an image with `--iso`, or a DVD DDP Image in that folder with `--ddp`. Use
  `--cache` to specify the location for converted assets, and `--audio-language` to specify the language (ISO 639-1) for
  audio tracks without a language tag.
- `bd-chapters` lists Blu-ray playlists (length, number of chapters, clips) and the chapters of the longest main
  playlist (start times and lengths).

  > **Details**
  >
  > - You can specify a disc in a drive (`D:`), a disc folder, its `BDMV` or `PLAYLIST` folder, or a single `.mpls`
  >   file.
  > - Use `--playlist 2` for other playlists, `--all` for chapters of all playlists, `--json` for all data in seconds as
  >   JSON, and `--ogm` for the OGM chapter file format (`CHAPTER01=00:00:00.000`).
  > - This is useful when placing chapter points at the same positions as on a Blu-ray with the same video.
  > - Only playlists are read (playlists on encrypted discs are not encrypted, but their video cannot be used).
- `disc-info` outputs information reported by the drive about the disc in the recorder: media IDs for DVD+R and DVD+RW,
  whether it is blank, appendable, or closed, the number of sessions, start and size per track, whether space is
  reserved, if it is empty, the next write position, the last written sector, and for dual-layer discs, layer 0 (a
  previous interrupted write might exist on a blank DVD+R DL). Nothing is written. Use this to check what remains on a
  disc that failed to write (Windows. `recorders` and `burn` are described in [Building](14-build.md#14-building)).
