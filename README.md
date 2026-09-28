# SpotyPop

A Spotify mini player for the panel of the [COSMIC™](https://system76.com/cosmic) desktop.

- Shows the current song in the panel, with its cover or a live audio scope.
- Play, pause, skip, seek, shuffle, repeat, volume and like from a popup.
- Search Spotify, browse your playlists and podcasts, and manage the queue.
- Pick which Spotify Connect device plays.
- Optionally turns this computer into a Spotify Connect device, so music plays
  without the Spotify app open.

## Install

SpotyPop is distributed through the COSMIC Flatpak repository, the same one
the COSMIC Store uses for applets. Search for **SpotyPop** in the COSMIC Store,
or install it from a terminal:

```sh
flatpak remote-add --if-not-exists --user cosmic https://apt.pop-os.org/cosmic/cosmic.flatpakrepo
flatpak install --user cosmic io.github.gbazan92.SpotyPop
```

Then add **SpotyPop** to the panel from COSMIC Settings → Desktop → Panel → Applets.

You need Spotify Premium: the Web API only controls playback for Premium accounts.

## Connect your account

SpotyPop talks to Spotify with a developer app of your own, which is free:

1. Create an app at [developer.spotify.com/dashboard](https://developer.spotify.com/dashboard).
2. Under **Redirect URIs** add `http://127.0.0.1:8888/callback`, check **Web API** and save.
3. Copy the app's Client ID, paste it in the popup and press **Connect**.

To play on this computer, press **Link** in the popup and approve once in the
browser.

## Contributing

Bug reports and pull requests are welcome at
[github.com/gbazan92/spotypop](https://github.com/gbazan92/spotypop/issues).

### Build from source

You need a COSMIC desktop, [Rust](https://rustup.rs) (stable),
[`just`](https://github.com/casey/just), `pkg-config` and the PulseAudio headers
for the local receiver (PipeWire serves the same API):

```sh
sudo apt install just pkg-config libpulse-dev
git clone https://github.com/gbazan92/spotypop.git
cd spotypop
just install   # release build, installed to ~/.local (no sudo)
```

Add **SpotyPop** to the panel as above. `just uninstall` removes it.

### Working on it

```sh
just check   # clippy, pedantic
just test
just dev     # install and restart only the applet, not the whole panel
just restart-player   # after changing the receiver in player/
```

- `src/`: the applet. `ui.rs` draws the panel and popup, `window.rs` holds the
  state, `spotify/` is the Web API client.
- `player/`: `spotypop-player`, the local Spotify Connect receiver (librespot).
- `i18n/en/spotypop.ftl`: every user-facing string ([Fluent](https://projectfluent.org/)).
- `res/`: desktop entry, AppStream metainfo and icons.

### Flatpak

`io.github.gbazan92.SpotyPop.yml` builds offline and sandboxed, the way the
COSMIC Flatpak repository does. It needs `org.flatpak.Builder` from Flathub:

```sh
just flatpak-sources   # after any Cargo.lock change: regenerates cargo-sources.json
just flatpak           # builds and writes spotypop.flatpak
just flatpak-lint
```

## Credits

SpotyPop is an adaptation for COSMIC of two MIT-licensed projects made for
[Omarchy](https://github.com/basecamp/omarchy). Many thanks to their authors:

- [omarchy-spotify](https://github.com/ninepointlabs/omarchy-spotify) by
  **Ninepoint Labs**: the panel and popup design, and the Spotify Web API
  client this applet ports to Rust.
- [Omarchy-Spotify](https://github.com/stappmus/Omarchy-Spotify) by
  **Kristoffer Haugland** ([@stappmus](https://github.com/stappmus)) and
  contributors: the local Spotify Connect receiver `spotypop-player` is adapted
  from its backend, and plays through his fork of
  [librespot](https://github.com/librespot-org/librespot).

Their license notices are in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## License

SpotyPop is licensed under the [GNU General Public License v3.0](LICENSE).

Spotify is a trademark of Spotify AB. COSMIC is a trademark of System76, Inc.
SpotyPop is an independent project, not affiliated with or endorsed by either.
