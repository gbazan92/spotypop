name := 'cosmic-ext-applet-spotify'
player := 'cosmic-ext-spotify-player'
appid := 'io.github.gbazan92.CosmicExtAppletSpotify'

bin-dir := env('HOME') / '.local/bin'
desktop-dst := env('HOME') / '.local/share/applications' / appid + '.desktop'
icon-src := 'res/icons/hicolor/scalable/apps' / appid + '-symbolic.svg'
icon-dst := env('HOME') / '.local/share/icons/hicolor/scalable/apps' / appid + '-symbolic.svg'
# pkill -x only sees the first 15 characters of a name, too few for the receiver
player-pid := env('XDG_RUNTIME_DIR', '/run/user/' + `id -u`) / name / 'player.pid'

default: build

# The receiver needs the PulseAudio headers (PipeWire serves the same API)
deps:
    sudo apt install libpulse-dev

build:
    cargo build --release --workspace

check:
    cargo clippy --workspace --all-targets -- -W clippy::pedantic

test:
    cargo test --workspace

# Installs to ~/.local so the panel can find it (no sudo needed)
install: build
    install -Dm0755 target/release/{{name}} {{bin-dir}}/{{name}}
    install -Dm0755 target/release/{{player}} {{bin-dir}}/{{player}}
    sed 's|^Exec=.*|Exec={{bin-dir}}/{{name}}|' res/{{appid}}.desktop | install -Dm0644 /dev/stdin {{desktop-dst}}
    install -Dm0644 {{icon-src}} {{icon-dst}}

uninstall:
    pkill -F {{player-pid}} -f {{player}} 2>/dev/null || true
    rm -f {{bin-dir}}/{{name}} {{bin-dir}}/{{player}} {{desktop-dst}} {{icon-dst}}

# Restarts the panel to load the new binary; the receiver keeps playing
reload:
    pkill -x cosmic-panel || true

# Restarts the receiver too, after installing a new build of it
restart-player:
    pkill -F {{player-pid}} -f {{player}} 2>/dev/null || true

dev: install reload

clean:
    cargo clean
