name := 'cosmic-ext-applet-hola'
appid := 'io.github.gbazan92.CosmicExtAppletHola'

bin-dst := env('HOME') / '.local/bin' / name
desktop-dst := env('HOME') / '.local/share/applications' / appid + '.desktop'

default: build

build:
    cargo build --release

check:
    cargo clippy -- -W clippy::pedantic

# Installs to ~/.local so the panel can find it (no sudo needed)
install: build
    install -Dm0755 target/release/{{name}} {{bin-dst}}
    sed 's|^Exec=.*|Exec={{bin-dst}}|' res/{{appid}}.desktop | install -Dm0644 /dev/stdin {{desktop-dst}}

uninstall:
    rm -f {{bin-dst}} {{desktop-dst}}

# Restarts the panel to load the new binary
reload:
    pkill -x cosmic-panel || true

dev: install reload

clean:
    cargo clean
