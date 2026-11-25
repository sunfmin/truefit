#!/bin/sh
# Checks `release.sh cut` without building or publishing (gate.sh runs it):
# cargo, lipo, rustup and gh are faked, origin is a bare repo here. The cut
# dispatches the tap's workflow, waits for that run and fails with its log
# when it fails; a tag it pushed is gone from origin when it ends, and a tag
# origin already had on HEAD stays.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
mkdir -p "$t/bin" "$t/repo/packaging"
cp "$here/release.sh" "$t/repo/packaging/"
cp "$here/../Cargo.toml" "$t/repo/"
cat >"$t/bin/cargo" <<'EOF'
#!/bin/sh
for a; do case "$prev" in --target) target=$a ;; esac; prev=$a; done
mkdir -p "target/$target/release"
printf '#!/bin/sh\necho "truefit %s"\n' "$TRUEFIT_RELEASE" >"target/$target/release/truefit"
chmod +x "target/$target/release/truefit"
EOF
cat >"$t/bin/lipo" <<'EOF'
#!/bin/sh
case "$1" in -archs) echo "x86_64 arm64" ;; -create) cp "$4" "$3" ;; esac
EOF
printf '#!/bin/sh\n' >"$t/bin/rustup"
# gh: `workflow run` makes run <n+1>, which `run view` reports as $RUN_ENDS
# after failing with HTTP 500 as many times as $s.fails says. `run cancel`
# is logged with whether origin still had the tag then.
cat >"$t/bin/gh" <<'EOF'
#!/bin/sh
s=$FAKE_GH
case "$1 $2" in
"workflow run") echo "$*" >>"$s.calls"; echo $(( $(cat "$s.n") + 1 )) >"$s.n" ;;
"run list") n=$(cat "$s.n"); [ "$n" = 0 ] || echo "$n" ;;
"run cancel")
    there=$(git ls-remote "$FAKE_ORIGIN" refs/tags/v1.2.3 | cut -f1)
    echo "run cancel $3 tag:${there:+there}" >>"$s.calls" ;;
"run view")
    case "$*" in *--log-failed*) echo "the run's failed log"; exit 0 ;; esac
    f=$(cat "$s.fails" 2>/dev/null || echo 0)
    if [ "$f" -gt 0 ]; then
        echo $((f - 1)) >"$s.fails"
        echo "HTTP 500: Internal Server Error" >&2
        exit 1
    fi
    echo "$RUN_ENDS" ;;
esac
EOF
chmod +x "$t/bin/"*
git init -q --bare "$t/origin.git"
cd "$t/repo"
git init -q && git config user.email t@t && git config user.name t
git add . && git commit -q -m one
git remote add origin "$t/origin.git"
head=$(git rev-parse HEAD)

run_cut() {
    PATH="$t/bin:$PATH" FAKE_GH="$t/gh" FAKE_ORIGIN="$t/origin.git" RELEASE_POLL=0 \
        RELEASE_WAIT_LIMIT="${LIMIT:-20}" RUN_ENDS="$1" \
        packaging/release.sh cut v1.2.3 >"$t/out" 2>&1
}
fail() { echo "release-test: $*" >&2; cat "$t/out" >&2; exit 1; }
tagged() { git ls-remote "$t/origin.git" refs/tags/v1.2.3 | cut -f1; }

echo 0 >"$t/gh.n"
run_cut "completed success" || fail "a run that succeeds fails the cut"
grep -qx "workflow run release.yml -R sunfmin/homebrew-tap -f repo=truefit -f tag=v1.2.3" "$t/gh.calls" \
    || fail "the cut ran '$(cat "$t/gh.calls")'"
[ -z "$(tagged)" ] || fail "the tag the cut pushed is still on origin"
[ -z "$(git tag)" ] || fail "the cut left a tag here: factorium's own tag step would fail"

if run_cut "completed failure"; then fail "a run that fails passes the cut"; fi
grep -q "the run's failed log" "$t/out" || fail "a failed run's log is not printed"
[ -z "$(tagged)" ] || fail "the tag of a failed cut is still on origin"

# A transient API error while the run is fine does not end the cut (truefit#35).
echo 2 >"$t/gh.fails"
run_cut "completed success" || fail "two HTTP 500s from gh run view fail the cut"
[ "$(cat "$t/gh.fails")" = 0 ] || fail "the cut did not poll past the errors"
[ -z "$(tagged)" ] || fail "the tag the cut pushed is still on origin"

# The limit passes with the run queued: the run is cancelled while the tag is
# still there, then the tag is taken away.
: >"$t/gh.calls"
if LIMIT=0 run_cut "queued "; then fail "a run still queued at the limit passes the cut"; fi
grep -q "still 'queued '" "$t/out" || fail "the cut does not say the run is still queued"
n=$(cat "$t/gh.n")
grep -qx "run cancel $n tag:there" "$t/gh.calls" \
    || fail "run $n was not cancelled before the tag went: '$(cat "$t/gh.calls")'"
[ -z "$(tagged)" ] || fail "the tag of a timed-out cut is still on origin"

# factorium's tag step, after the cut: it tags HEAD and pushes the tag. Then a
# cut run again finds it there, and leaves it.
git tag -a v1.2.3 -m v1.2.3
git push -q origin refs/tags/v1.2.3
git tag -d v1.2.3 >/dev/null
run_cut "completed success" || fail "a tag origin has on HEAD fails the cut"
[ -n "$(tagged)" ] || fail "the cut took away a tag it did not push"

git commit -q --allow-empty -m two
if run_cut "completed success"; then fail "a tag origin has on another commit passes the cut"; fi
grep -q "not HEAD" "$t/out" || fail "the cut does not say the tag is on $head"
echo "release-test: ok"
