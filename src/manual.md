# `truefit`

Keeps a system's tests and models true to the real system.

You (the agent) test a system through stand-ins of the programs it drives,
simulate it under seeded faults, and model its design. Each is only as good as
its tie to the real thing: a stand-in that answers as the real program does
not makes every test pass while the real system breaks. truefit holds those
ties, and tells you what to do where one is missing or stale.

truefit is installed as a Homebrew formula (`brew install sunfmin/tap/truefit`;
factorium cuts each version and upgrades it). `truefit --version` names the
release, `truefit X.Y.Z`, as its tag `vX.Y.Z`; a build that is not a release
says where it was built, `truefit X.Y.Z+<commits>.g<commit>[.dirty]`: the
nearest version tag (or, without one, Cargo.toml's version), the commits since
it, and the commit.

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
  version are written to `.truefit/<name>.json`. When a case differs, its
  answers are written to `.truefit/<name>.differ.json`, until a real run
  finds no case differing and removes it. Commit `.truefit/`, so the
  contract's freshness and its differences are in git and not on one host.
- A real program truefit cannot run here (not found, or its `ready` command
  fails: no login, no account, no device) is `not run`, with what it `needs`.
  It is never a pass: run it where it can run.

Run from the directory holding `truefit.toml`; command paths with a `/` in
them are relative to it, others are looked up on PATH.

### `truefit.toml`

```toml
[contract]
standins = ["tests/fake-*", "tests/fakes"] # where the repo's stand-ins live
fresh_days = 30                       # a passing real run older is stale (default 30)

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
agrees, a wrong one that differs, and one no contract declares.

## `truefit sim`

Seeded runs of the system's own simulation; a failing seed is kept and
reruns exactly.

- `truefit sim [--seeds N | --budget <seconds>]`: reruns every kept seed
  first, as regressions, then runs new seeds: `[sim] seeds` of them, `N`, or
  as many as fit in the budget (at least one). Each new seed that fails is
  run a second time and kept in `.truefit/seeds/<seed>.json` with its verdict
  and every line it printed; each is printed with its `rerun` command. Commit
  `.truefit/seeds/`: kept seeds are the repo's, not one host's.
- `truefit sim --seed <s>`: reruns one seed. A kept one is compared with its
  kept lines. When they differ it runs once more: two reruns that differ are
  `nondeterministic`, a failure, with `differs`, the first line they differ
  at (`line`, `then`, `now`); two that agree mean the system's code changed
  since, and they replace the kept lines (`changed` says where). A seed not
  kept runs as a new one.
- A kept seed that passes again stays kept, rerun on every `truefit sim`.
  `truefit sim --forget <s> --why "<reason>"` removes one that passed when
  last run; commit that with the reason. A seed that still fails is never
  forgotten: fix what it reaches.
- A run whose last line is no verdict is `broken`: not kept, and it stops
  the new seeds, as it would break on every one.

It exits 0 when every seed passed, 1 when one fails, is nondeterministic or
broken. Each report has the seed's `result` (`passes`, `fails`,
`nondeterministic`, `broken`), its `verdict`, its `steps` and `rerun`.

### `[sim]` in `truefit.toml`

```toml
[sim]
run = ["tests/sim"] # the simulation's command line; truefit appends the seed
seeds = 20          # new seeds per `truefit sim` (default 20)
```

truefit runs it from the directory holding `truefit.toml`. New seeds are
decimal numbers below 2^32; one given to `--seed` may be any letters,
digits, `-` and `_`.

### What a simulation is

truefit does not write it: the system's repo does, in its own language,
running its own code. From the seed alone it:

- runs the system's own code step by step against stand-ins (with
  contracts, so they answer as the real programs do), the order of events
  and the faults (a crash, a lost message, a timeout) chosen from the seed;
- has one clock, which only the simulation moves, and one random source,
  seeded by the seed: no wall time, process ids, threads' order or a hash
  map's order, or its runs of one seed differ;
- has a round it can step: each step one event, delivered to the system's
  code as the real world would deliver it;
- checks the invariants after every step, each with one home the system's
  own checks (and its model) use too, not a copy for the simulation;
- prints one JSON line per step, `{"step": 3, "event": "...", "state": {...}}`
  with the state the invariants read, and a final verdict line:
  `{"verdict": "pass"}`, or `{"verdict": "fail", "invariant": "<name>",
  "step": 3}`. A panic of the system's code is caught and printed as a
  failed invariant, so the run still ends with its verdict.

`testdata/sim/` is a worked example: a queue whose worker can crash between
applying a message and acking it, with a bug some seeds reach (a crashed
message is applied twice), and `sim-nondeterministic`, the same made
nondeterministic on purpose (a restarted worker named by its process id).

## `truefit next [--all]`

What to do about the repo's ties: the next task (`task`), or with `--all`
every one (`tasks`), each with its `kind`, `summary`, `files`, `evidence`,
`steps` and the `accept` command that holds it done; `counts` per kind,
`notes` on what was not looked for, and `by`, the truefit that read them. Do
one task by its steps, run its accept command, commit, then `truefit next`
again. No task is an answer: every tie is fresh. It exits 0 with tasks or
without, 2 on a fault in `truefit.toml`. It runs each stand-in's `version`
command, never a case, and reads kept seeds without running the simulation.

- `no-contract`: a file under `[contract] standins` (paths, or globs whose
  `*` and `?` match within one path part; a directory stands for its files)
  that no `[[standin]]`'s `run` names. Where the stand-ins live is declared,
  not guessed: without `standins` none is looked for, and `notes` says so.
- `stale-contract`: a declared stand-in whose last passing real run
  (`.truefit/<name>.json`) is missing, older than `fresh_days`, or against
  another version than its `version` command answers here now.
- `differing-case`: the last real run found cases whose answers differ
  (`.truefit/<name>.differ.json`, with both answers): make the stand-in
  answer as the real program does.
- `nondeterministic-sim`: kept seeds whose two runs differed, each with the
  first line they differ at: make the simulation choose all from the seed.
- `failing-seed`: a kept seed that failed when last run: fix the bug it
  reaches.

The answer has the shape of `slimfit next --all`'s, so a factorium
`[idle]` tasks line files its tasks as issues, one open issue per kind:
`tasks = "slimfit --version && slimfit next --all && truefit next --all"`.

## To come

- `truefit model`: the system's models, and its real runs checked as traces of
  them.
