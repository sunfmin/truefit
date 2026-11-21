# Issue tracker: factorium

Issues for this repo live in factorium's local tracker on this host, not in
GitHub Issues: factorium does not read GitHub. `<checkout>` is this repo's
name in `factorium checkout list` (`truefit`).

- Read: `factorium issue list truefit`, `factorium issue show truefit <n>`
- File: `factorium issue add truefit <title> --body-file - [--blocked-by <n> …]`.
  Give dependencies when filing: an issue with no blocker is ready, and a
  worker takes it on the next tick.
- Comment: `factorium issue comment truefit <n> "<text>"`
- Dependencies: `factorium issue block truefit …`

There are no labels: whether an issue can start is decided only by its
blockers.
