# Rusty Dock

A desktop dock for Windows, written in Rust with [Slint](https://slint.dev/). It shows one icon per
installed game launcher (Steam, Battle.net, EA, plus your own custom launchers). Clicking a launcher
opens a popup listing its installed games, and clicking a game launches it through its launcher.

## Building

```sh
make windows   # cross-compiles with MinGW and copies RustyDock.exe to the project root
cargo test     # unit tests
```

## License

Rusty Dock is free software: you can redistribute it and/or modify it under the terms of the GNU
General Public License as published by the Free Software Foundation, version 3. See
[LICENSE](LICENSE).

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without
even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

## Third-party components

- **UI toolkit**: [Slint](https://slint.dev/), used under the GNU GPL v3.
