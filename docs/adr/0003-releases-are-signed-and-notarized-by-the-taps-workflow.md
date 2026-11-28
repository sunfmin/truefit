# 3. Releases are signed and notarized by the tap's workflow

Status: accepted, 2026-11-22, after slimfit ADR 0013 (below).

## Context

Until this decision a cut (`packaging/release.sh cut`) built on the cutting host,
for aarch64 only, and published from there: the archive to the tap's release,
the formula (with `depends_on arch: :arm64`) to the tap. The binary was only ad
hoc signed: Intel Macs could not run it, and Gatekeeper rejected a copy
downloaded in a browser.

The owner, 2026-11-22: 「把所有的工具都变成 universal。」「把公开的工具全部在
GitHub CI 上签。」「这个密钥的环境变量可以统一配置到账号上面吗？」 A personal account
has no account-wide secrets. slimfit decided the same question first:
[slimfit ADR 0013](https://github.com/sunfmin/slimfit/blob/main/docs/adr/0013-releases-are-signed-and-notarized-by-the-taps-workflow.md).
The contract is the tap's
[`.github/workflows/README.md`](https://github.com/sunfmin/homebrew-tap/blob/main/.github/workflows/README.md).

## Decision

truefit follows slimfit ADR 0013 as it stands; this records only what is
truefit's. Invariant: **the signing identity and the notary key live only in
sunfmin/homebrew-tap, and only its `release` workflow signs.** sunfmin/truefit
holds no signing secret and no release workflow.

- **truefit builds.** `packaging/release.sh build <vX.Y.Z> <outdir>` builds
  `aarch64-apple-darwin` and `x86_64-apple-darwin` with `TRUEFIT_RELEASE` set,
  checks `truefit --version` says the tag's version, `lipo -create`s them, and
  leaves the universal `truefit` and `truefit.rb` in `<outdir>`: no
  `depends_on arch`, the license as `Cargo.toml` states it, `url` and `sha256`
  as `@URL@` and `@SHA256@` for the tap. The gate's `dry` runs the same build.
- **The tap signs and publishes** the release `truefit-v<X.Y.Z>`
  (`truefit-<X.Y.Z>-macos-universal.tar.gz`) and `Formula/truefit.rb`.
- **The cut waits.** `cut` builds here first, pushes the tag, runs
  `gh workflow run release.yml -R sunfmin/homebrew-tap -f repo=truefit -f
  tag=<tag>`, and waits for that run, bounded by `RELEASE_WAIT_LIMIT` (2400 s
  from the cut's start; `[cut] limit` is 2700 s). A failed run fails the cut
  with its `--log-failed`. A tag the cut pushed is taken away when it ends, as
  factorium's own tag step fails on a tag origin already has.
  `packaging/release-test.sh` (in the gate) checks this with `gh`, `cargo` and
  `lipo` faked.

## Consequences

- The tap's workflow needed no change for truefit.
- The gate builds two release targets instead of one host build.
- A cut needs only `gh` logged in as a user who may push truefit and dispatch
  the tap's workflow; no tap token on the host.
