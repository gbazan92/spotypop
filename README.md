# SpotyPop

A Spotify mini player for the [COSMIC](https://system76.com/cosmic) panel.

- Shows the current song in the panel, with its cover or a live audio scope.
- Play, pause, skip, seek, shuffle, repeat, volume and like from a popup.
- Search Spotify, browse your playlists and podcasts, and manage the queue.
- Pick which Spotify Connect device plays.
- Optionally turns this computer into a Spotify Connect device, so music plays
  without the Spotify app open.

The interface is in English and Spanish, following the desktop language.

## Requirements

- COSMIC desktop
- Spotify Premium (the Web API only controls playback for Premium accounts)
- A Spotify developer app of your own (free), for its Client ID
- To build: Rust, [`just`](https://github.com/casey/just) and the PulseAudio
  headers (`sudo apt install libpulse-dev`; PipeWire serves the same API)

## Install from source

```sh
just install   # builds and installs to ~/.local
```

Then add **SpotyPop** to the panel from COSMIC Settings → Desktop → Panel → Applets.

## Connect your account

1. Create an app at [developer.spotify.com/dashboard](https://developer.spotify.com/dashboard).
2. Under **Redirect URIs** add `http://127.0.0.1:8888/callback`, check **Web API** and save.
3. Copy the app's Client ID, paste it in the popup and press **Connect**.

To play on this computer, press **Link** in the popup and approve once in the
browser.

## Development

```sh
just check   # clippy, pedantic
just test
just dev     # install and restart the applet
```

Translations live in `i18n/<language>/spotypop.ftl` ([Fluent](https://projectfluent.org/)).

## License

GPL-3.0
