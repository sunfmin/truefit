#!/bin/sh
# The gate: what must pass before anything lands.
set -eu
cd "$(dirname "$0")"
[ -d "$HOME/.cargo/bin" ] && PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all --check
cargo clippy --all-targets --quiet -- -D warnings
# The tests, measured against .slimfit/baseline.json (in place of cargo test).
slimfit check
echo "gate: ok"
