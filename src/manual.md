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

## `truefit model`

The system's models (TLA+), checked by TLC, and its runs checked as traces
of them.

- `truefit model [<name>]`: checks every declared model (or one) with TLC,
  each within its `limit`. Each report has its `result` (`passes`, `fails`,
  `timed out`, `broken`), `states` (distinct) and `depth`, and `seconds`. A
  model that fails has TLC's `error` (`Invariant AppliedOnce is violated.`)
  and the `behaviour` that reaches it: each state with the `action` that
  took it there and its variables. One that is `broken` (TLC could not parse
  or evaluate it) or `timed out` has the `output` TLC printed from its first
  error. Cheap when the bounds are small: put it in the gate.
- `truefit model [<name>] --trace <file>`: checks a run's records as a
  behaviour of the model that declares a `[model.trace]` mapping (name it
  when more than one does). The records are the file's JSON lines that have
  a `step` (the simulation's step lines, or a real run's in the same shape;
  other lines are skipped), or the `lines` of a kept seed
  (`.truefit/seeds/<seed>.json`). From the model's initial state, each
  record must be one step of its next-state action, with every mapped
  variable equal to the record's; unmapped variables may take any value the
  model allows (TLC tries each). `allowed`: the model has a behaviour that is
  this run. `disallowed`: the first `record` no step of the model reaches
  (`step`, the record's own), with the last allowed record, `after` (null:
  the first record is disallowed). `broken`: TLC could not evaluate the
  mapping on a record (`output`: a field the record lacks, say). The trace
  check asks only whether the model allows the run; the model's properties
  are `truefit model`'s.

It exits 0 when every model passes (or the trace is allowed), 1 when not.

TLC is the one release truefit pins, its `tla2tools.jar` checked against
the pinned SHA-256 (each report's `tlc` names both, and the jar). truefit
downloads it once from the official release into
`~/.local/share/truefit/tlc/<version>/`; set `TRUEFIT_TLC_FROM` to a URL (or
`file://` path) of a copy of that release where GitHub is out of reach: one
whose checksum differs is refused. TLC runs on `java` (11 or later) on PATH.
It runs in a temporary directory; nothing is written in the repo.

### `[[model]]` in `truefit.toml`

```toml
[[model]]
name = "queue"             # what `truefit model <name>` names
spec = "models/Queue.tla"  # relative to truefit.toml; modules it EXTENDS beside it
config = "models/Queue.cfg" # TLC's config: CONSTANTS, SPECIFICATION, INVARIANT ...
limit = 60                 # seconds TLC may take on it (default 300)
init = "Init"              # the initial predicate and next-state action
next = "Next"              #   a trace is checked against (these are the defaults)
# The coverage record: the code each action (or property) stands for, and
# the commit at which you last confirmed the model describes it.
commit = "42a65a58620cc50dc65af3c1ed5884a887984259"
covers = [
  { action = "ApplyAck", file = "src/worker.rs", function = "Worker::ack" },
]

# The model's variables, each a TLA+ expression over one record `r`
# (JSON objects are records, arrays sequences; other numbers and null are
# strings, and keys that are no TLA+ name are left out).
[model.trace]
applied = "r.state.applied"
head = "r.state.next"
```

A trace's check keeps the config's CONSTANT sections and replaces the rest.

### What to model

truefit does not write the model; the system's repo does. Model the design
where a bug costs the most and tests reach the least: what runs at once or
in an order the system does not choose (workers, retries, crashes and
restarts, messages lost or delivered twice, a lease that expires), where
every order of events must keep a promise (exactly once, no lost update,
one leader, a queue that drains). Not what one function computes: a test
or a verifier on the code does that better.

- One action for each step the code takes as one indivisible step, named
  as the code names it; a step the code can be interrupted in the middle
  of is two actions. Each action is in `covers`, with the file and
  function it stands for.
- Variables for the state those steps read and write, no more; constants
  for the sizes (messages, workers), as small as still finds the bugs
  (two or three of each, mostly). A model TLC cannot finish within its
  `limit` needs smaller bounds, not a larger limit.
- The faults the real world brings (a crash between two steps, a message
  lost, a timeout) as actions of their own.

### How to write its properties

- An invariant states, of every state, the promise in one sentence of the
  system's words (`AppliedOnce == \A m \in 1..N : applied[m] <= 1`: no
  message is applied twice), and has one home: the simulation's and the
  code's own checks state the same, by the same name.
- A type invariant (`TypeOK`) catches a modelling error early; it is not
  the promise.
- A property you have not seen fail proves nothing: break the model as the
  code could break (drop a check, swap two steps) and see TLC find it, then
  put it back.
- When TLC finds a behaviour that breaks a property, read it in the
  system's words: a bug in the design (fix the code and the model alike)
  or a modelling error (fix the model). Never weaken the property to pass.
- Map the records the simulation (and the real system) prints to the
  model's variables in `[model.trace]`, and check its runs with
  `--trace`: a run the model does not allow means the model and the code
  disagree, at that step.

Where pstack's `create-formal-verification-skill` is installed
(`~/.claude/skills/` or `~/.agents/skills/`), run
`/create-formal-verification-skill` to choose the target and write the
model with it, then declare that model here; its
`maintain-formal-verification-skill` brings one back in line.

`testdata/sim/` is a worked example: `Queue.tla`, the queue's design (the
worker marks a message done when it applies it, so a redelivery is not
applied again), passes; the simulation's runs are its traces. Seed 1 runs
every message once, `allowed`; seed 3 crashes after applying message 2 and
applies it again when it is redelivered, `disallowed` at step 3:

```sh
./sim 1 > ok.jsonl  && truefit model --trace ok.jsonl   # allowed
./sim 3 > bad.jsonl && truefit model --trace bad.jsonl  # disallowed, step 3
```

## `truefit next [--all]`

What to do about the repo's ties: the next task (`task`), or with `--all`
every one (`tasks`), each with its `kind`, `summary`, `files`, `evidence`,
`steps` and the `accept` command that holds it done; `counts` per kind,
`notes` on what was not looked for, and `by`, the truefit that read them. Do
one task by its steps, run its accept command, commit, then `truefit next`
again. No task is an answer: every tie is fresh. It exits 0 with tasks or
without, 2 on a fault in `truefit.toml`. It runs each stand-in's `version`
command, never a case, reads kept seeds without running the simulation, and
compares models' coverage with git, never running TLC.

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
- `stale-model`: a model whose covered files (`covers`) changed since the
  `commit` its coverage record names, or whose `commit` this repo's history
  lacks: bring the model in line with the code, check it, and move `commit`
  forward. Outside a git repo it is not looked for, and `notes` says so.

The answer has the shape of `slimfit next --all`'s, so a factorium
`[idle]` tasks line files its tasks as issues, one open issue per kind:
`tasks = "slimfit --version && slimfit next --all && truefit next --all"`.
