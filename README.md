# Frostpane

Frostpane puts weather on your Windows desktop. Snow settles on window tops
and the taskbar. There is also rain with the odd thunderstorm, cherry blossom,
autumn leaves, fireflies and shooting stars.

While it snows, you can do these things with the mouse:

- Roll snowballs and throw them.
- Stack snowballs into snowmen and dress them up.
- Plant decorated trees and hang decorations.

Now and then Santa, a snowplough, a yeti or a pair of quarrelling snowmen drop
by.

Frostpane lives in the system tray. The weather layer never takes your clicks.

## Features

- Snow builds up on windows and the taskbar, and drifts move with the windows.
- Rain, sakura petals, autumn leaves, fireflies and starfall, with light and
  heavy variants.
- Optional "follow the weather outside" mode. It uses
  [Open-Meteo](https://open-meteo.com/) for a city you pick, and needs no
  account and no location services.
- Matching wallpapers that can change with the time of day.
- Multi-monitor and per-monitor DPI support.
- Available in many languages.

## Privacy

- There are no accounts, no analytics, no telemetry and no crash uploads.
- Frostpane makes only two kinds of network request:
  - The weather lookup for the city you chose. It is off until you turn it on.
  - A check for new releases. If one exists, Frostpane shows a notice. It never
    updates itself.

## Install

Download the latest release from the
[Releases](https://github.com/duruer/frostpane/releases) page. Builds are
available for Windows x64, x86 and ARM64.

## Building from source

Requirements:

- Windows 11.
- A recent stable Rust toolchain, installed with
  [rustup](https://rustup.rs/).
- The MSVC build tools.

```
cargo build --release
target\release\frostpane.exe
```

Run the tests with:

```
cargo test --workspace
```

Command-line options:

| Option | Effect |
| --- | --- |
| `--quiet` | Starts Frostpane in the tray. |
| `--seconds N` | Exits after N seconds (for testing). |

Set `FROSTPANE_LOG=debug` for verbose logs.

## Contributing

Use [Conventional Commits](https://www.conventionalcommits.org/) for commit
messages, for example `fix: ...` or `feat: ...`. Releases are generated from
these messages.

## License

Frostpane is free software. You can redistribute it and modify it under the
terms of the GNU General Public License, version 3 or later. See
[`LICENSE`](LICENSE).

Weather data is by [Open-Meteo](https://open-meteo.com/) under
CC BY 4.0.
