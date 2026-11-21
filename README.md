# truefit

Keeps a system's tests and models true to the real system.

truefit is made for AI coding agents. A system's tests run against stand-ins
of the programs it drives, a simulation runs it under seeded faults, and a
model checks its design; each is only as good as its tie to the real thing.
truefit holds those ties and tells the agent what to do where one is missing
or stale.

The manual is `truefit --help`, and it is the only copy.

## Develop

- `./gate.sh`: fmt, clippy, the tests through `slimfit check`, and the release
  script as far as it goes without publishing (`packaging/release.sh dry`).
- A release is cut by factorium (`[cut]` in `factorium.toml`):
  `packaging/release.sh cut vX.Y.Z` publishes the formula `sunfmin/tap/truefit`.

The decisions are in `docs/adr/`.
