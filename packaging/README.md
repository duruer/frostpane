# Packaging

Scripts live in `scripts/`; the CI release job calls them.

| Script | Output (in `dist/`) |
|---|---|
| `scripts/package-macos.sh` | `frostpane-vX.Y.Z-macos-universal.dmg`, `.zip` (unsigned, ad-hoc signed universal `Frostpane.app`) |
| `scripts/package-linux.sh [x64\|arm64]` | `frostpane-vX.Y.Z-linux-<arch>.tar.gz`, `.deb`, `.AppImage` |

Files:

- `macos/Info.plist`: bundle template (`@VERSION@` is filled in). `LSUIElement` makes it a menu bar agent with no Dock icon.
- `icons/frostpane.svg`: master artwork (1024x1024). Also copied to `docs/assets/logo.svg` (README logo).
- `icons/png/`, `windows/frostpane.ico`, `macos/Frostpane.icns`, `linux/icons/frostpane-{16,32,48,64,128,256,512}.png`: generated from the SVG. Regenerate after editing it (needs the `resvg` CLI, `cargo install resvg --locked`, and Pillow):

  ```
  python -I -X utf8 packaging/icons/render-icons.py
  cp packaging/icons/frostpane.svg docs/assets/logo.svg
  ```

  The `.ico` is embedded in `frostpane.exe` by `app/frostpane/build.rs`; `fp-ui` embeds `icons/png/frostpane-{64,128,256,512}.png` (window icon, onboarding logo, home header).
- `linux/frostpane.desktop`: menu entry, shipped in the tarball, deb and AppImage.
- `deb/control.in`: control file template for the hand-built `.deb` (`dpkg-deb`, no cargo-deb needed).

Linux packaging needs `dpkg-deb`, `binutils`, `curl` and `desktop-file-utils` (appimagetool validates the .desktop file).

Environment knobs: `SKIP_BUILD=1` (use existing `target/` binaries), `FORMATS="tar.gz deb appimage"` (Linux subset), `APPIMAGETOOL=/path` (use a local appimagetool).
