# DiscCraft app icon

A **beaver**, seated and looking at the viewer: an engraving portrait on a full-bleed blue tile with rounded
corners, laid out as the other Crafting Apps' icons.

## Geometry

A 1024 px tile, rounded square, no border. The macOS renders pad it onto Apple's 824/1024 icon grid; the
Windows and Linux renders use the whole tile, as the other Crafting Apps do.

## Provenance

The owner's original picture, made with Google Gemini's image generation (2026-11-20). License: see
`LICENSE.txt`.

## Files

- `disccraft.png`: canonical master (1024 px). There is no vector master, so there is no hicolor scalable icon.
- `disccraft-1024.png`: 1024 px render on the macOS grid.
- `disccraft.icns`: macOS bundle icon (`CFBundleIconFile`), 16–1024 px on the macOS grid.
- `disccraft.ico`: Windows icon, 16–256 px, embedded in the `.exe` by `apps/disccraft/build.rs` and used by the
  installer and its shortcuts.
- `hicolor/<size>/apps/io.github.mOqOm.DiscCraft.png` (16–512): Linux icon theme.

The app also sets the window icon (the 256 px PNG) and the Wayland app ID / X11 window class
(`io.github.mOqOm.DiscCraft`, as in the desktop file) at runtime (`apps/disccraft/src/main.rs`).

## Regenerate

Replace `disccraft.png`, then regenerate the other files with the development tools of the source repository. Each size is reduced from the master with a Lanczos 3
filter, the alpha premultiplied so the see-through corners do not darken the edges.
