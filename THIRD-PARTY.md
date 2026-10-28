# Third-party notices

zenix itself is licensed **GPL-3.0-or-later** — the full text is in [LICENSE](LICENSE).

This file lists the third-party components that are redistributed with, or
statically linked into, the zenix binary, together with their licenses. The
packaged `.deb` ships this file, the project `LICENSE` and the license texts
named below under `/usr/share/doc/zenix/`.

## Redistributed as separate files

| Component | Version | License | Shipped as |
|---|---|---|---|
| [herdr](https://github.com/herdrdev/herdr) | 0.9.5 (pinned release binary, `vendor/herdr-pins.tsv`) | Apache-2.0 | `/usr/share/zenix/herdr` — the seed the client copies to `~/.local/bin/herdr` on first run; fetched by `scripts/fetch-herdr.sh`, license text at `licenses/herdr/LICENSE` |

## Statically linked (vendored)

| Component | License | Notices |
|---|---|---|
| herdr-protocol (`vendor/herdr-protocol/`) | Apache-2.0 | `LICENSE-APACHE`, `NOTICE.md` |
| herdr-client (`vendor/herdr-client/`) | Apache-2.0 | `LICENSE-APACHE`, `NOTICE.md` |

Both are copies of the corresponding herdr-gpui crates, kept as **local source**
(they may be edited when a daemon change needs it); `vendor/HERDR_UPSTREAM.md`
records the upstream commit and the resync command, and the `NOTICE.md` files
name what was adapted upstream and at which revision.

## Statically linked component library

The shell, the terminal's chrome, the settings surface and the chat panel are
built on the **ely** component library
(<https://github.com/ELY-Open-Source/Ely-GPUI-Components>), pinned at commit
`30c66fe` (`file:///home/zero/zero/up/Ely-GPUI-Components`; the pin is kept
reachable by the library checkout's `zenix-pin-30c66fe` branch).

| Crates | Version | License | Shipped as |
|---|---|---|---|
| `ely-core`, `ely-chat`, `ely-agent`, `ely-settings` (direct dependencies) and their transitive `ely-canvas`, `ely-charts`, `ely-collab`, `ely-documents`, `ely-editor`, `ely-git` | 0.1.1 / `ely-core` 0.2.0 | MIT | compiled into the binary; license text at `licenses/ely/LICENSE`, shipped to `/usr/share/doc/zenix/ely/LICENSE` |

The library builds on **GPUI** — the crates.io `gpui-pre*` 0.3.10 snapshot —
which is Apache-2.0 and is statically linked as well.

The library's assets also carry its typefaces in the binary. zenix installs no
font of its own; its root names the library's UI face once so inherited text
follows Inter, and the terminal names the mono face for its cell grid. Each
face is OFL-1.1, and its license text travels with the package:

| Font | License | Shipped as |
|---|---|---|
| Inter (UI face) | SIL OFL-1.1 | embedded by `ely_core::Assets`; text at `licenses/ely/Inter-OFL.txt` → `/usr/share/doc/zenix/ely/Inter-OFL.txt` |
| JetBrains Mono (mono face, the terminal's) | SIL OFL-1.1 | embedded by `ely_core::Assets`; text at `licenses/ely/JetBrainsMono-OFL.txt` → `/usr/share/doc/zenix/ely/JetBrainsMono-OFL.txt` |
| IBM Plex Sans (the library's SVG text face) | SIL OFL-1.1 | embedded by `ely_core::Assets`; text at `licenses/ely/IBMPlexSans-OFL.txt` → `/usr/share/doc/zenix/ely/IBMPlexSans-OFL.txt` |

## Cargo dependencies

The build resolves **595 normal dependencies** — 583 from crates.io, the 10
`ely-*` crates from the component-library checkout above, and the two vendored
herdr crates. The Linux dependency graph resolves the same crate set (596
entries once zenix itself is counted). All are under permissive licenses — MIT,
Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, Unlicense, CC0-1.0, 0BSD,
MPL-2.0, CDLA-Permissive-2.0 — which are compatible with GPL-3.0-or-later.

Two dependencies need an explicit election:

- `self_cell` — `Apache-2.0 OR GPL-2.0-only`. zenix takes **Apache-2.0**; the
  `GPL-2.0-only` branch is not used, as it is incompatible with GPL-3.0.
- `ryu` — `Apache-2.0 OR BSL-1.0`. zenix takes **Apache-2.0**.

Reproduce the inventory with (the `sed` collapses `cargo tree`'s repeated
`(*)` markers so each package is counted once):

```bash
cargo tree --target x86_64-unknown-linux-gnu -e normal --prefix none -f "{p}|{l}" \
  | sed 's/ (\*)$//' | sort -u
```
