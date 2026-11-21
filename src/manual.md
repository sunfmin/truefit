# `truefit`

Keeps a system's tests and models true to the real system.

You (the agent) test a system through stand-ins of the programs it drives,
simulate it under seeded faults, and model its design. Each is only as good as
its tie to the real thing: a stand-in that answers as the real program does
not makes every test pass while the real system breaks. truefit holds those
ties, and tells you what to do where one is missing or stale.

## Output

Every command prints one JSON object on stdout with a `next` field: what to do
next. Exit 0: everything passed. Exit 1: something failed or was not run; each
part says which, and `next` says what to do. Exit 2: the command could not
finish; it prints `{"error": [{"fault": "...", "fix": "..."}, ...]}`, every
fault it found with what fixes it. Do every fix, then run it again.

## `truefit contract`

Each declared stand-in and its real program answer the same cases alike.

- `truefit contract`: runs every case against the stand-ins alone. Cheap: put
  it in the gate. A stand-in that cannot run one of its declared cases fails.
  Each stand-in's `real_run` is its last passing real run, or null: never.
- `truefit contract --real [<name>]`: runs every case (of one stand-in, or of
  all) against the stand-in and its real program, and compares their answers.
  Never in a gate: the real program may be slow, costly or absent. A case
  whose answers differ fails, and prints both (`standin`, `real`) and which
  parts `differ`. When every case agrees, the date and the real program's
  version are written to `.truefit/<name>.json`: commit it, so the contract's
  freshness is in git and not on one host.
- A real program truefit cannot run here (not found, or its `ready` command
  fails: no login, no account, no device) is `not run`, with what it `needs`.
  It is never a pass: run it where it can run.

Run from the directory holding `truefit.toml`; command paths with a `/` in
them are relative to it, others are looked up on PATH.

### `truefit.toml`

```toml
[[standin]]
name = "git"                          # what `--real <name>` names
run = ["tests/fake-git"]              # the stand-in's command line
real = ["git"]                        # the real program's
needs = "git 2.x on PATH"             # what a real run needs (default: `real` on PATH)
ready = ["git", "config", "user.name"] # must succeed, or the real run is not run
version = ["git", "--version"]        # its first line is kept as the version
compare = ["exit", "stdout", "stderr", "files"] # default: exit and stdout
normalise = [{ pattern = '\d{4}-\d\d-\d\d', replace = "<date>" }]

[[standin.case]]
name = "status-clean"                 # each case: a command line, stdin, environment
dir = "testdata/clean-repo"           # copied into a fresh directory it runs in
args = ["status", "--porcelain"]      # appended to `run` and to `real`
stdin = ""
env = { GIT_PAGER = "cat" }
files = ["out.txt"]                   # files it writes there, compared under `files`
```

Both sides run each case in their own fresh directory. That directory's path
becomes `<dir>` in their answers; then each `normalise` rule (a regex and its
replacement) is applied to stdout, stderr and the files, on both sides alike.
Normalise only what differs from run to run (dates, temporary paths, ids,
versions), never what the system decides on.

### What a contract covers

Every answer of the real program that the system under test reads: each
command line it runs, each output it parses, each exit code it branches on,
each file it reads back, its errors as well as its successes. Read the
system's code where it calls the program, not the stand-in: a case for each
call site and each answer it tells apart. Compare the parts it reads, and no
more. A behaviour the system relies on that no case holds is untested
against reality; when one breaks in production, add the case first.

`testdata/plutil/` is a worked example: a stand-in of macOS `plutil` that
agrees, and a wrong one that differs.

## To come

- `truefit next`: the next task, with its evidence and how to accept it.
- `truefit sim`: seeded runs of the system's own simulation; a failing seed
  reruns exactly.
- `truefit model`: the system's models, and its real runs checked as traces of
  them.
