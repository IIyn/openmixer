# OpenMixer

<p align="center">
  <img src="packaging/openmixer.svg" alt="OpenMixer logo" width="128">
</p>

A mixer for **Audient iD** audio interfaces on Linux, written in Rust with [gpui](https://crates.io/crates/gpui).
It drives the interface's internal DSP the same way the official iD software does:
Main / Cue A / Cue B mixes, pan, mute/solo, stereo links, a monitor section
(Dim, Cut, Mono, Ø), headphone level, output routing and level meters.

## Supported models

| Model | USB | Status |
|---|---|---|
| iD14 MKII | 2708:0008 | protocol verified: all controls are written, and the saved state is pushed on connect |
| iD24 | 2708:000d | protocol verified (no routing in the UI yet) |
| iD14 | 2708:0002 | not verified: nothing is sent on connect, only your manual changes are written |
| iD44 / iD44 MKII | 2708:0005 / 000b | read-only: the mixer layout is not confirmed yet |

Writing to the wrong matrix cell can mute an interface until it is unplugged,
so unverified models are handled cautiously.

## No kernel driver needed

These interfaces are *USB Audio Class 2* compliant, so `snd-usb-audio` already
handles audio. OpenMixer talks to the mixer through the interface's spare DFU
interface, because the firmware accepts mixer requests on any of its
interfaces. It **never** detaches `snd-usb-audio`, so audio keeps playing
while the mixer runs.

> Tools that detach interface 0 (the "exclusive" mode of MixiD/BiD) make the
> sound card disappear until the interface is replugged.

## Installation

Download the file for your distribution from the
[latest release](https://github.com/IIyn/openmixer/releases/latest):

| Distribution | File | Install |
|---|---|---|
| Debian, Ubuntu, Mint, Pop!_OS… | `openmixer_<ver>-1_amd64.deb` | `sudo apt install ./openmixer_*.deb` |
| Fedora, openSUSE, RHEL… | `openmixer-<ver>-1.x86_64.rpm` | `sudo dnf install ./openmixer-*.rpm` |
| Any distribution | `openmixer-<ver>-x86_64.AppImage` | `chmod +x openmixer-*.AppImage` and run it |
| Arch and others | `openmixer-<ver>-x86_64-unknown-linux-gnu.tar.gz` | extract, then `./install-desktop.sh ./openmixer` |

The .deb and .rpm packages install the udev rule for you. With the AppImage
or the tarball, install it once (it gives the logged-in user USB access to
the interface):

```sh
sudo install -m644 70-openmixer-audient.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger --attr-match=idVendor=2708
```

### From source

```sh
sudo install -m644 packaging/70-openmixer-audient.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger --attr-match=idVendor=2708

cargo build --release
packaging/install-desktop.sh      # icon + application launcher for the current user
```

### Updates

**Settings → Updates** checks the GitHub releases and shows their release
notes. The AppImage and the standalone binary update themselves; the .deb
and .rpm packages are updated by your package manager.

Commands:

- `openmixer`: graphical mixer. Closing the window keeps OpenMixer in the system tray;
  left-click the tray icon to reopen it, or use its menu to quit. Launching it again
  while it runs just shows the existing window
- `openmixer --hidden`: start in the system tray only. The **Start at login** button in the
  app adds an autostart entry (`~/.config/autostart/openmixer.desktop`) that uses it
- `openmixer --preview`: preview the mixer without an interface connected
- `openmixer probe`: read-only. Prints the model, monitor volume and meters
- `openmixer fix-balance [--remove]`: left/right balance fix (see below)
- `openmixer udev-rule`: prints the udev rule and how to install it

Mixer state is saved in `~/.config/openmixer/<model>.toml`.

## Known Linux issues

**Sound only on the left.** There are two causes:
1. The interface's DSP matrix keeps its previous mix, and no Linux tool sets
   it. On connect, OpenMixer writes a clean mix: DAW 1 left, DAW 2 right,
   at 0 dB.
2. The firmware ignores the left half of the *hardware* volume that PipeWire
   uses, which can leave the right side at -127 dB. OpenMixer resets both
   monitor outputs to 0 dB on connect. The **L/R balance fix** button (or
   `openmixer fix-balance`) switches Audient outputs to software volume in
   WirePlumber, so this does not happen again.

**Microphone not detected.** PipeWire's "HiFi" profile splits the interface
into mono sources, `Mic1` and `Mic2`. Set the default source to the one that
matches the input your microphone is plugged into. OpenMixer's meters show
where the signal arrives. Condenser microphones also need **48V** phantom
power.

## Releasing

Publish a GitHub release with a tag such as `v0.2.0`. The
[release workflow](.github/workflows/release.yml) sets the crate version
from the tag, runs the tests, and attaches the AppImage, .deb, .rpm,
tarball and `SHA256SUMS` to the release.

## Building

gpui 0.2.2 depends on `xattr 0.2.3`, which does not build with `libc` ≥ 0.2.190.
`Cargo.lock` pins `libc` to 0.2.188, so avoid a blanket `cargo update`.

The code forbids `unsafe` and denies `unwrap`, `expect` and `panic!`
(see `[lints]` in `Cargo.toml`).

## Credits

The protocol comes from the reverse-engineering work of
[MixiD](https://github.com/TheOnlyJoey/MixiD), [BiD](https://github.com/baakhoff/BiD)
and [Monix](https://github.com/sKuhLight/monix) (all MIT).

Audient and iD are trademarks of Audient Ltd. OpenMixer is an independent
project, not affiliated with or endorsed by Audient.

## License

OpenMixer is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free
Software Foundation, either version 3 of the License, or (at your option)
any later version. See [LICENSE](LICENSE).
