name := 'spotypop'
player := 'spotypop-player'
appid := 'io.github.gbazan92.SpotyPop'

bin-dir := env('HOME') / '.local/bin'
desktop-dst := env('HOME') / '.local/share/applications' / appid + '.desktop'
icon-src := 'res/icons/hicolor/scalable/apps'
icon-dst := env('HOME') / '.local/share/icons/hicolor/scalable/apps'
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
    install -Dm0644 {{icon-src}}/{{appid}}.svg {{icon-dst}}/{{appid}}.svg
    install -Dm0644 {{icon-src}}/{{appid}}-symbolic.svg {{icon-dst}}/{{appid}}-symbolic.svg

uninstall:
    pkill -F {{player-pid}} -f {{player}} 2>/dev/null || true
    rm -f {{bin-dir}}/{{name}} {{bin-dir}}/{{player}} {{desktop-dst}} {{icon-dst}}/{{appid}}.svg {{icon-dst}}/{{appid}}-symbolic.svg

# Regenerates the offline crate list the Flatpak build needs; run after any Cargo.lock change
flatpak-sources:
    #!/usr/bin/env bash
    set -euo pipefail
    tools="${XDG_CACHE_HOME:-$HOME/.cache}/spotypop-flatpak-tools"
    mkdir -p "$tools"
    curl -fsSL -o "$tools/flatpak-cargo-generator.py" https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
    flatpak run --filesystem="$tools" --filesystem="$PWD" --share=network --command=sh org.freedesktop.Sdk//25.08 -c \
        "python3 -m pip install -q --target '$tools/py' aiohttp tomlkit PyYAML && PYTHONPATH='$tools/py' python3 '$tools/flatpak-cargo-generator.py' Cargo.lock -o cargo-sources.json"

# Builds the Flatpak offline, the way the COSMIC Flatpak repository does, and bundles it
flatpak:
    flatpak run org.flatpak.Builder --user --sandbox --force-clean --install-deps-from=flathub --repo=flatpak-repo flatpak-build {{appid}}.yml
    flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo flatpak-repo {{name}}.flatpak {{appid}}

flatpak-lint:
    flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest {{appid}}.yml || true
    flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo flatpak-repo || true

# Restarts only this applet. Killing cosmic-panel drops the whole bar, and
# the session is slow to bring it back. The panel respawns this process.
# Match the executable, not the command line: a pkill -f of this path also
# hits the shell that is running the recipe.
reload:
    #!/usr/bin/env bash
    set -euo pipefail
    target='{{bin-dir}}/{{name}}'
    for dir in /proc/[0-9]*; do
        exe=$(readlink "$dir/exe" 2>/dev/null || true)
        # After install the running binary is the same path with " (deleted)".
        if [ "$exe" = "$target" ] || [ "$exe" = "$target (deleted)" ]; then
            kill "${dir##*/}" 2>/dev/null || true
        fi
    done

# Restarts the receiver too, after installing a new build of it
restart-player:
    pkill -F {{player-pid}} -f {{player}} 2>/dev/null || true

dev: install reload

clean:
    cargo clean
