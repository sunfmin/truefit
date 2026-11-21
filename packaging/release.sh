#!/bin/sh
# One version of truefit, cut on this Mac: factorium.toml's cut step runs
# `packaging/release.sh cut $FACTORIUM_VERSION` in a worktree at the base's
# tip, then factorium tags what it left at HEAD and `brew upgrade`s the
# formula.
#
#   release.sh cut <vX.Y.Z>   build for Apple Silicon with the tag's version
#                             stamped (build.rs, TRUEFIT_RELEASE; Cargo.toml
#                             is not edited and nothing is committed), check
#                             `truefit --version` equals the tag, package,
#                             and publish: the archive to the tap's release
#                             truefit-v<X.Y.Z>, the formula to the tap's
#                             Formula/truefit.rb
#   release.sh check-tag <vX.Y.Z>  exit 0 if cut would take the version, 2 if not
#   release.sh dry            what the gate runs: check the binary the gate
#                             built (target/release/truefit) says the
#                             development form X.Y.Z+<commits>.g<commit>[.dirty],
#                             package it at that version, check the formula,
#                             publish nothing
#
# The archive is downloaded from the tap (sunfmin/homebrew-tap), as every
# formula there is. DIST names another directory than dist/.
set -eu
cd "$(dirname "$0")/.."
PATH="$PATH:$HOME/.cargo/bin"
export PATH

REPO=https://github.com/sunfmin/truefit
TAP=sunfmin/homebrew-tap
TARGET=aarch64-apple-darwin
DIST=${DIST:-dist}

usage() {
    echo "usage: release.sh cut <vX.Y.Z> | dry | check-tag <vX.Y.Z>" >&2
    exit 2
}

# check_binary <path> <version>: the binary says the version being cut.
check_binary() {
    said=$("$1" --version)
    [ "$said" = "truefit $2" ] || {
        echo "release: $1 --version says '$said', not 'truefit $2'" >&2
        exit 1
    }
}

# package <binary> <version>: the archive and the formula, in $DIST.
package() {
    mkdir -p "$DIST"
    archive=truefit-$2-macos-arm64.tar.gz
    tar -czf "$DIST/$archive" -C "$(dirname "$1")" truefit
    sha=$(shasum -a 256 "$DIST/$archive" | cut -d' ' -f1)
    release=truefit-v$2
    formula=$DIST/truefit.rb
    # The crate's license, as Cargo.toml states it: the formula says the same.
    license=$(sed -n 's/^license = "\(.*\)"$/\1/p' Cargo.toml | head -n1)
    [ -n "$license" ] || {
        echo "release: Cargo.toml states no license" >&2
        exit 1
    }
    cat > "$formula" <<FORMULA
class Truefit < Formula
  desc "Keeps a system's tests and models true to the real system"
  homepage "$REPO"
  license "$license"
  url "https://github.com/$TAP/releases/download/$release/$archive"
  sha256 "$sha"
  version "$2"

  depends_on :macos
  depends_on arch: :arm64

  def install
    bin.install "truefit"
  end

  test do
    assert_equal "truefit #{version}", shell_output("#{bin}/truefit --version").strip
  end
end
FORMULA
    ruby -c "$formula" >/dev/null
    grep -qx "  license \"$license\"" "$formula" || {
        echo "release: $formula does not declare license \"$license\"" >&2
        exit 1
    }
}

# check_tag <tag>: a version tag vX.Y.Z, or usage.
check_tag() {
    case "$1" in
    v*[!0-9.]* | v*..* | v.* | v*.) usage ;;
    v[0-9]*.[0-9]*.[0-9]*) ;;
    *) usage ;;
    esac
    case "$1" in v*.*.*.*) usage ;; esac
}

case "${1:-}" in
check-tag)
    [ $# -eq 2 ] || usage
    check_tag "$2"
    exit 0
    ;;
dry)
    [ $# -eq 1 ] || usage
    built=target/release/truefit
    version=$("$built" --version | sed 's/^truefit //')
    case "$version" in
    *[!0-9a-z.+]* | *+*+*) dev=no ;;
    [0-9]*.[0-9]*.[0-9]*+[0-9]*.g[0-9a-f]*) dev=yes ;;
    *) dev=no ;;
    esac
    [ "$dev" = yes ] || {
        echo "release: $built --version says '$version', not X.Y.Z+<commits>.g<commit>[.dirty]" >&2
        exit 1
    }
    package "$built" "$version"
    echo "release: dry ok (truefit $version, $DIST/$archive)"
    exit 0
    ;;
cut) [ $# -eq 2 ] || usage ;;
*) usage ;;
esac
check_tag "$2"
version=${2#v}

# The tag's version, stamped by build.rs: the tag goes on HEAD as it is.
TRUEFIT_RELEASE=$version cargo build --release --locked -p truefit --target "$TARGET"
built=target/$TARGET/release/truefit
check_binary "$built" "$version"
package "$built" "$version"

# A release that is there already takes the archive again: a cut that
# failed half way is run a second time.
if gh release view "$release" -R "$TAP" >/dev/null 2>&1; then
    gh release upload "$release" -R "$TAP" --clobber "$DIST/$archive"
else
    gh release create "$release" -R "$TAP" --title "truefit $version" \
        --notes "The binary the formula \`truefit\` installs." "$DIST/$archive"
fi
tap=$(mktemp -d)
gh repo clone "$TAP" "$tap" -- --quiet --depth 1
cp "$formula" "$tap/Formula/truefit.rb"
git -C "$tap" add Formula/truefit.rb
if git -C "$tap" diff --cached --quiet; then
    echo "release: the tap's formula is truefit $version already"
else
    git -C "$tap" commit --quiet -m "truefit $version"
    git -C "$tap" push --quiet origin HEAD
fi
rm -rf "$tap"
