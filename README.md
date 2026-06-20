# HT32 Panel

[![CI](https://github.com/tekgnosis-net/ht32-panel/actions/workflows/ci.yml/badge.svg)](https://github.com/tekgnosis-net/ht32-panel/actions/workflows/ci.yml)
[![Release](https://github.com/tekgnosis-net/ht32-panel/actions/workflows/release.yml/badge.svg)](https://github.com/tekgnosis-net/ht32-panel/actions/workflows/release.yml)

![Panel Face](https://raw.githubusercontent.com/tekgnosis-net/ht32-panel/main/images/ht32-panel-ascii-landscape.png)

Front-panel display and LED control for mini PCs with HT32-based LCD and RGB LEDs ([Skullsaints Agni](https://www.electroniksindia.com/products/agni-by-skullsaints-mini-pc-intel-twin-lake-n150-vibrant-lcd-screen-m-2-ssd-mini-tower-with-rgb-lights-wifi-6-4k-uhd-dual-lan-for-home-and-office), [AceMagic S1](https://acemagic.com/products/acemagic-s1-12th-alder-laker-n95-mini-pc), etc.).

This is an independent, **headless-first** continuation of [ananthb/ht32-panel](https://github.com/ananthb/ht32-panel): the panel runs as a robust system service on servers and mini-PCs and is managed entirely from a browser — no desktop session required.

## Components

- **Daemon** (`ht32paneld`): D-Bus system service that drives the LCD and RGB LEDs, with a built-in HTMX web UI
- **CLI** (`ht32panelctl`): D-Bus client for scripting and daemon control
- **Web UI**: monitor and control the panel from any browser

## What's different in this fork

- **Self-healing USB link** — the daemon detects device write failures and transparently reconnects, instead of silently dropping to the firmware's default screen until the service is restarted.
- **Typed-widget Layout engine** — faces are described as typed zones (text / bars / sparklines) with per-zone redraw cadence: the foundation for partial-screen updates and a future template builder.
- **Headless-focused packaging** — deb/rpm ship the daemon, systemd unit, udev rule, and D-Bus system policy with no GUI/GTK dependencies. The desktop tray applet and AppImage have been retired.

## Install

See the [installation guide](https://tekgnosis-net.github.io/ht32-panel/install.html) for all options (deb, rpm, NixOS, from source).

### Quick start

```bash
# Debian/Ubuntu
curl -LO https://github.com/tekgnosis-net/ht32-panel/releases/latest/download/ht32-panel_0.9.0_amd64.deb
sudo dpkg -i ht32-panel_*.deb
sudo apt update

# Fedora
curl -LO https://github.com/tekgnosis-net/ht32-panel/releases/latest/download/ht32-panel-0.9.0-1.x86_64.rpm
sudo dnf install ./ht32-panel-*.rpm

# NixOS
nix run github:tekgnosis-net/ht32-panel
```

The deb and rpm packages auto-configure a signed apt/dnf repository, so later updates arrive through your normal `apt upgrade` / `dnf upgrade`.

Then start the service:

```bash
sudo systemctl enable --now ht32-panel.service
```

## Documentation

- [Installation](https://tekgnosis-net.github.io/ht32-panel/install.html)
- [Configuration](https://tekgnosis-net.github.io/ht32-panel/config.html)
- [API Reference](https://tekgnosis-net.github.io/ht32-panel/api/)

## Acknowledgements

- Forked from [ananthb/ht32-panel](https://github.com/ananthb/ht32-panel) by Ananth Bhaskararaman.
- Protocol reverse-engineering and source ideas from [tjaworski/AceMagic-S1-LED-TFT-Linux](https://github.com/tjaworski/AceMagic-S1-LED-TFT-Linux/commit/2971f2b0703bd3170a3f714867652f7e085ec447).

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).

Copyright &#169; 2026 Ananth Bhaskararaman (original work) and tekgnosis-net (fork).
