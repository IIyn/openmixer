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

Every [release](https://github.com/IIyn/openmixer/releases/latest) ships
four formats. Pick the one for your distribution; `<ver>` is the version
number (e.g. `0.2.0`).

| Distribution | File |
|---|---|
| Debian, Ubuntu, Linux Mint, Pop!_OS, elementary… | `openmixer_<ver>-1_amd64.deb` |
| Fedora, openSUSE, RHEL, Nobara… | `openmixer-<ver>-1.x86_64.rpm` |
| Any distribution, no install | `openmixer-<ver>-x86_64.AppImage` |
| Arch, CachyOS, Manjaro, others | `openmixer-<ver>-x86_64-unknown-linux-gnu.tar.gz` |

Only x86_64 (64-bit Intel/AMD) builds are published for now.

### Debian / Ubuntu (.deb)

```sh
sudo apt install ./openmixer_<ver>-1_amd64.deb
```

That's all: the package installs the USB access rule, the launcher and the
icon. Start **OpenMixer** from the application menu, then plug in (or
replug) the interface. Update by installing the next .deb the same way.

### Fedora / openSUSE (.rpm)

```sh
sudo dnf install ./openmixer-<ver>-1.x86_64.rpm      # Fedora, RHEL, Nobara
sudo zypper install ./openmixer-<ver>-1.x86_64.rpm   # openSUSE
```

Same as the .deb: everything is set up, start it from the application menu.

### AppImage (any distribution)

1. Give USB access to your user, once ([why](#no-kernel-driver-needed)):

   ```sh
   echo 'SUBSYSTEM=="usb", ATTR{idVendor}=="2708", MODE="0660", GROUP="audio", TAG+="uaccess"' \
     | sudo tee /etc/udev/rules.d/70-openmixer-audient.rules >/dev/null
   sudo udevadm control --reload && sudo udevadm trigger --attr-match=idVendor=2708
   ```

2. Make the file executable and run it:

   ```sh
   chmod +x openmixer-<ver>-x86_64.AppImage
   ./openmixer-<ver>-x86_64.AppImage
   ```

Keep the AppImage somewhere stable (e.g. `~/Applications/`): **Settings →
Start at login** and the in-app updater use its location. Tools such as
Gear Lever or AppImageLauncher can add it to the application menu.

### Tarball (Arch, CachyOS and others)

1. Extract it and install the binary and the launcher for your user:

   ```sh
   mkdir openmixer && tar -xzf openmixer-<ver>-x86_64-unknown-linux-gnu.tar.gz -C openmixer
   cd openmixer
   install -Dm755 openmixer ~/.local/bin/openmixer
   ./install-desktop.sh ~/.local/bin/openmixer
   ```

   `~/.local/bin` must be in your `PATH` (it is on most distributions).

2. Give USB access to your user, once:

   ```sh
   sudo install -m644 70-openmixer-audient.rules /etc/udev/rules.d/
   sudo udevadm control --reload && sudo udevadm trigger --attr-match=idVendor=2708
   ```

Start **OpenMixer** from the application menu. It updates itself from
**Settings → Updates**.

### After installing

- If OpenMixer says *USB access denied*, the udev rule is missing or the
  interface was plugged in before it was installed: replug it.
- On the first launch, OpenMixer writes a clean mix to the interface
  (computer playback at 0 dB on both sides, inputs closed).
- Enable **Settings → L/R balance fix** if one side is quieter or silent.

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
