<div align="center">

  <img width="3852" height="1204" alt="srtwm-logo-gray-green" src="https://github.com/user-attachments/assets/bf8e13f9-2975-491e-abc8-8a3fd245b5f2" />

</div>

<br>

<div align="center">

# srtwm

**A tiling window manager for X11, written in Rust.**

Windows tile in a binary tree. Every new window splits the focused one.
Floats, workspaces, a resize mode, and a Unix-socket control protocol.

<br>

[![tests](https://img.shields.io/badge/tests-55_passing-3fb950?style=flat-square)](.)
&nbsp;·&nbsp;
[![rust](https://img.shields.io/badge/rust-2024-dea584?style=flat-square)](.)
&nbsp;·&nbsp;
[![x11](https://img.shields.io/badge/x11-x11rb-0071c5?style=flat-square)](.)
&nbsp;·&nbsp;
[![license](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)

</div>

<br>



https://github.com/user-attachments/assets/495985fe-5f7d-4525-9aea-47a394e87377



<br>

## Install

```sh
git clone https://github.com/SulphShock/srtwm
cd srtwm
cargo build --release
sudo install -Dm755 target/release/srtwm    /usr/local/bin/srtwm
sudo install -Dm755 target/release/srtwmctl /usr/local/bin/srtwmctl
```

Add to `~/.xinitrc`:

```sh
exec srtwm
```

**Try it without installing.** Runs in a nested X server, safe to poke at:

```sh
./scripts/dev-xephyr.sh
```

<br>

## Configure

`~/.config/srtwm/config.toml`. Every field is optional.

```toml
[general]
mod     = "Super"
terminal = "kitty"
gap      = 8
border   = 2

[colors]
focused   = "#88c0d0"
unfocused = "#3b4252"

[binds]
"Mod+Return" = "spawn:kitty"
"Mod+q"      = "close"
"Mod+f"      = "toggle-fullscreen"
"Mod+r"      = "mode:resize"
"Mod+h"      = "focus:left"
"Mod+l"      = "focus:right"

[mode.resize]
"h"      = "resize:left:20"
"l"      = "resize:right:20"
"Escape" = "mode:normal"
```

Reload with `Mod+Shift+r`.

<br>

## Keybindings

`<Mod>` is whatever you set `general.mod` to. Defaults use vim keys.

| | |
|--|--|
| `Mod+h j k l` | move focus |
| `Mod+Shift+h j k l` | swap window |
| `Mod+Return` | spawn terminal |
| `Mod+q` | close focused |
| `Mod+f` | toggle fullscreen |
| `Mod+1..9` | switch workspace |
| `Mod+Shift+1..9` | send window to workspace |
| `Mod+r` | enter resize mode |

<br>

## Control socket

`srtwmctl` talks to the running WM over a Unix socket at
`$XDG_RUNTIME_DIR/srtwm.sock`. One JSON object per line.

```sh
srtwmctl focus left        # same as Mod+h
srtwmctl workspace 3
srtwmctl query tree        # JSON dump of the layout
srtwmctl subscribe         # stream every event
```

```json
$ srtwmctl query tree
{
  "workspace": 0,
  "windows": [
    { "window": 2097153, "x": 8, "y": 8, "width": 632, "height": 704,
      "floating": false, "fullscreen": false, "focused": true }
  ]
}
```

Good enough to drive a status bar.

<br>

## How it's built

Split into a pure core and a thin X11 layer. The core compiles and tests
green without a display.

```
src/
  geometry.rs   Rect, Axis, Dir
  actions.rs    the Action enum and its parser
  config.rs     the TOML schema
  ipc.rs        JSON-line Unix socket

  layout/
    bsp.rs      the binary tree — 19 tests
    mod.rs      tree + gap -> rects

  state.rs      windows, docks, workspaces
  window.rs     the Client struct
  mouse.rs      drag to move, drag to resize

  ewmh.rs       _NET_* atoms, struts, titles
  keys.rs       key grabs from the modifier map
  query.rs      the three JSON queries
  event.rs      the event match
  wm.rs         the main loop
  main.rs       connect, load config, run
```

**55 tests** in the pure core. No `x11rb` in the test build.

<br>

## What it doesn't do

srtwm is small on purpose.

- **No multi-monitor.** One screen only.
- **No window rules.** Can't pin mpv to a workspace from config.
- **No bar.** Bring your own, feed it `srtwmctl subscribe`.
- **No visible border colors** without a compositor — picom works.

The [roadmap](https://github.com/SulphShock/srtwm/issues) has the rest.

<br>

## Credits

Built on [x11rb](https://github.com/psychon/x11rb),
[xkeysym](https://github.com/psychon/x11rb),
[nix](https://github.com/nix-rust/nix),
[serde](https://serde.rs), and [toml](https://github.com/toml-rs/toml).

The window-manager shape owes a debt to
[i3](https://i3wm.org), [bspwm](https://github.com/baskerville/bspwm), and
[dwm](https://dwm.suckless.org).

<br>

## License

MIT.

<br>

<div align="center">
<sub>Built with ☕ and a healthy disrespect for <code>XErrorEvent</code>.</sub>
</div>
