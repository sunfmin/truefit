#!/bin/sh
# The gate: what must pass before anything lands.
set -eu
cd "$(dirname "$0")"
[ -d "$HOME/.cargo/bin" ] && PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all --check
cargo clippy --all-targets --quiet -- -D warnings
# The tests, measured against .slimfit/baseline.json (in place of cargo test).
slimfit check
# The release script, as far as it goes without publishing.
cargo build --release --quiet
sh -n packaging/release.sh
for good in v0.1.0 v1.22.333; do packaging/release.sh check-tag "$good"; done
for bad in "" v1 v1.2 1.2.3 v1.2.3.4 v1..3 v1.2.x "v1.2.3;x"; do
    if packaging/release.sh check-tag "$bad" 2>/dev/null; then echo "gate: release.sh takes \"$bad\""; exit 1; fi
done
mkdir -p .slimfit/out
DIST=.slimfit/out/dist packaging/release.sh dry
# Without tags (factorium's promotion clone), a build still stamps the
# development form: Cargo.toml's version + the commit count + the hash.
tagless=$PWD/.slimfit/out/tagless
rm -rf "$tagless"
git clone --quiet --no-tags . "$tagless"
CARGO_TARGET_DIR=$PWD/.slimfit/out/tagless-target cargo build --release --quiet --manifest-path "$tagless/Cargo.toml"
want="truefit $(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1)+$(git -C "$tagless" rev-list --count HEAD).g$(git -C "$tagless" rev-parse --short HEAD)"
said=$(.slimfit/out/tagless-target/release/truefit --version)
[ "$said" = "$want" ] || { echo "gate: a tagless clone says '$said', not '$want'"; exit 1; }
echo "gate: ok"
