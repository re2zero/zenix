# Third-party notices

zenix itself is licensed **GPL-3.0-or-later** — the full text is in [LICENSE](LICENSE).

This file lists the third-party components that are redistributed with, or
statically linked into, the zenix binary, together with their licenses. The
packaged `.deb` ships this file, the project `LICENSE` and the license texts
named below under `/usr/share/doc/zenix/`.

## Redistributed as separate files

| Component | Version | License | Shipped as |
|---|---|---|---|
| [herdr](https://github.com/ogulcancelik/herdr) | 0.9.3 (`herdr/` submodule, `7b116c05`) | Apache-2.0 | `/usr/share/zenix/herdr` — the seed the client copies to `~/.local/bin/herdr` on first run; license at `/usr/share/doc/zenix/herdr/LICENSE` |
| [Lilex](https://github.com/mishamyrt/Lilex) | 2.601 | SIL OFL-1.1 | `assets/fonts/*.ttf`, compiled into the binary with `include_bytes!`; license at `assets/fonts/OFL.txt` |

## Statically linked (vendored)

| Component | License | Notices |
|---|---|---|
| herdr-protocol (`vendor/herdr-protocol/`) | Apache-2.0 | `LICENSE-APACHE`, `NOTICE.md` |
| herdr-client (`vendor/herdr-client/`) | Apache-2.0 | `LICENSE-APACHE`, `NOTICE.md` |

Both are byte-identical copies of the corresponding herdr-gpui crates; their
`NOTICE.md` files record what was adapted upstream and at which revision.

## Cargo dependencies

The build resolves 846 normal dependencies (844 from crates.io plus the two
vendored crates above); 572 of those are in the Linux dependency graph. All are
under permissive licenses — MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib,
Unicode-3.0, Unlicense, CC0-1.0, 0BSD, MPL-2.0, CDLA-Permissive-2.0 — which are
compatible with GPL-3.0-or-later.

One dependency is dual-licensed and needs an explicit election:

- `self_cell` — `Apache-2.0 OR GPL-2.0-only`. zenix takes **Apache-2.0**; the
  `GPL-2.0-only` branch is not used, as it is incompatible with GPL-3.0.

Reproduce the inventory with:

```bash
cargo tree --target x86_64-unknown-linux-gnu -e normal --prefix none -f "{p}|{l}" | sort -u
```
