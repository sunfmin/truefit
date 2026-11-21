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
- A case whose own `ready` fails here (a fault the real program cannot be made
  to produce on this host: an unreachable network) is `not run`, with its
  `needs`; the other cases still run, but the real run is `not run` and
  records nothing.

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
faults = ["push-rejected", "unreachable"] # the failures it answers, each held by a case
fault_env = "FAKE_GIT_FAULT"          # tells the stand-in which (default TRUEFIT_FAULT)
# no_faults = { why = "...", sha256 = "..." } # instead: no call of it can fail usefully

[[standin.case]]
name = "status-clean"                 # each case: a command line, stdin, environment
dir = "testdata/clean-repo"           # copied into a fresh directory it runs in
args = ["status", "--porcelain"]      # appended to `run` and to `real`
stdin = ""
env = { GIT_PAGER = "cat" }
files = ["out.txt"]                   # files it writes there, compared under `files`

[[standin.case]]
name = "push-unreachable"
fault = "unreachable"                 # the stand-in runs with FAKE_GIT_FAULT=unreachable
args = ["push", "origin", "main"]     # the real side, never told, fails for real
ready = ["sh", "-c", "! curl -sm2 https://example.com"] # else this case is not run,
needs = "a host with no network"      #   with what it needs
```

Both sides run each case in their own fresh directory. That directory's path
becomes `<dir>` in their answers; then each `normalise` rule (a regex and its
replacement) is applied to stdout, stderr and the files, on both sides alike.
Normalise only what differs from run to run (dates, temporary paths, ids,
versions), never what the system decides on.

A stand-in's `faults` are the failures it can answer. Each is a name its
`fault_env` variable selects (how the stand-in reads it is the repo's own);
the stand-in side of a case with `fault` runs with it set, the real side
without, so the case passes only when the stand-in's failure answers as the
real program's does. Each declared fault needs at least one such case.

### What a contract covers

Every answer of the real program that the system under test reads: each
command line it runs, each output it parses, each exit code it branches on,
each file it reads back, its errors as well as its successes. Read the
system's code where it calls the program, not the stand-in: a case for each
call site and each answer it tells apart. Compare the parts it reads, and no
more. A behaviour the system relies on that no case holds is untested
against reality; when one breaks in production, add the case first.

Faults are found the same way: at each call site, read the error path. A
stand-in that can only succeed makes a test of every failure impossible to
write, and none is worse than a failure after a step that took effect
elsewhere (a token issued, a file written, a message sent). sesame#24:
its refresh drops a token the provider just issued when the store's write
fails, so a rotating provider's login is lost; its stand-in of `op`
answered no failed write, its contract held only the answers sesame tells
apart (and it treats every failed write alike), and line coverage counted
the save covered on success. For each call whose error the system handles,
or drops, after an earlier step took effect: declare the fault, a case
whose real side fails so, and the system's test that runs it. A stand-in
none of whose calls can fail usefully gets `no_faults`, with why and the
stand-in's SHA-256: it is not asked again until the stand-in changes.

`testdata/plutil/` is a worked example: a stand-in of macOS `plutil` that
agrees (with one fault, a write plutil refuses), a wrong one that differs
and declares no fault, and one no contract declares.

## `truefit sim`

Seeded runs of the system's own simulation; a failing seed is kept and
reruns exactly.

- `truefit sim [--seeds N | --budget <seconds>]`: reruns every kept seed
  first, as regressions, then runs new seeds: `[sim] seeds` of them, `N`, or
  as many as fit in the budget (at least one). `new.seeds` names every new
  seed it ran, each with its `result`, so a later step (a trace check of
  each) can run them again by `truefit sim --seed <s>`. Each new seed that
  fails is run a second time and kept in `.truefit/seeds/<seed>.json` with
  its verdict and every line it printed; `new.failing` holds its full
  report, printed with its `rerun` command. Commit
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
`nondeterministic`, `broken`), its `verdict`, its `steps` and `rerun`. A
plain `truefit sim` (kept seeds and new) also prints `reached`, each
invariant its steps named in `checked`, with how many seeds did and the
first, and writes it, with the date and the seeds run, to
`.truefit/sim.reached.json` (commit it): what `truefit promise` reads.

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
- prints one JSON line per step, `{"step": 3, "event": "...", "state": {...},
  "checked": ["applied-once"]}` with the state the invariants read and the
  invariants, by name, it checked in a state they are about (not where one
  holds vacuously: applied-once after a message's first delivery), and a
  final verdict line:
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

How to tell a promise that needs a model (and a simulation): it holds
across steps the system does not order itself. Ask of it: can two of the
system's processes, workers or sessions act on the same thing at once? Can
it crash or restart between two of the steps, and start again? Does it
retry, hold a lease or a lock that expires, or wait on a queue? Can a
message, an event or a command it sends be lost, delayed, reordered or
arrive twice? One yes is enough. A promise only one function's answer
keeps (a parse, a format, a computed limit) is a test's. The registry
(`truefit promise`) holds each promise to this: one of the concurrent kind
needs both `sim` and `model`.

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

## `truefit promise`

Each promise the system makes, tied to the check that keeps it. A system's
SPEC, README and ADRs make promises ("never", "exactly once", "at most
one", "within N seconds"); one no check keeps can break unseen.

- `truefit promise`: reports each `[[promise]]` by its `status`: `checked`
  (each named check reaches it), `unreached` (a named check never reached
  it: `why` names which), `unchecked` (no check, or too cheap a one without
  its reason), or `kept` (not checked, and why). And `candidates`: the
  sentences of `[promises] sources` with a promise word (never, always, at
  most, exactly once, only, within, every), `found` in all, those
  `unmarked`, and those marked a promise that no `[[promise]]` registers
  (`unregistered`); each with its `source` (file:line), `sentence`, `hash`
  and the `rung` its words point to (`test`, or `sim and model`). A
  candidate that holds a registered promise's `says` is that promise.
- `truefit promise --mark <hash> <promise|not> [<hash> <mark>]...`: marks
  candidates, kept in `.truefit/promises.json` by the sentence's hash, so
  one marked is not asked again; a sentence that changes has a new hash and
  is asked again. Commit it.

It exits 0 when every promise is checked or kept and every candidate is
marked and, if a promise, registered; 1 when not. It reads, never runs:
the last `truefit sim`, the models' configs.

### The ladder

A promise's checks, from the cheapest: `test` (a test of the system's
keeps it), `contract <stand-in>` (it rests on a real program's answers),
`sim` (the simulation checks it by name), `model <name>` (the model checks
it). Take the cheapest that can break when the promise does. A promise
about what runs at once or in an order the system does not choose (what
to model, above: its words say so, crash, retry, lease, queue, lost,
twice..., or `concurrent = true`) needs both `sim` and `model`; a cheaper
check on one, or `kept`, needs its reason (`why`, the `kept` reason).

A check is reached, or the promise is `unreached`:

- `sim`: a step of the last `truefit sim` (`.truefit/sim.reached.json`, no
  older than `[contract] fresh_days`) named the promise in its `checked`.
- `model <name>`: that model's config checks an invariant (or property) of
  the promise's name.
- `contract <stand-in>`: the stand-in declares a fault (or `no_faults` for
  it as it is now): through one that can only succeed, no test reaches the
  failures a promise is about. Its freshness is `truefit contract`'s.
- `test`: declared; truefit cannot see a test reach it.

Names are compared without case and punctuation: `applied-once` is the
model's `AppliedOnce`.

### `[[promise]]` in `truefit.toml`

```toml
[promises]
sources = ["SPEC.md", "README.md", "docs/adr/*.md"] # the default

[[promise]]
name = "applied-once"     # the invariant's one name: code, simulation, model
says = "A message is never applied twice." # the sentence, quoted
source = "SPEC.md:12"     # where the system makes it
check = ["sim", "model queue"] # any of test, contract <stand-in>, sim, model <name>
why = ""                  # the reason for a cheaper check on a concurrent one
# kept = "<why it is not checked>", in place of check
# concurrent = true: of the concurrent kind, though its words do not say so
```

`testdata/sim/` is a worked example: `SPEC.md`'s four promises, one checked
and reached (`applied-once`, after `truefit sim`), one registered to `sim`
and `model queue` that neither reaches (`no-lost-message`), one unchecked,
one kept with its reason; and one candidate to mark `not`.

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
- `standin-without-faults`: a declared stand-in with no `faults`, and no
  `no_faults` whose `sha256` is the stand-in's now (`evidence` has it): read
  the system's code at each call to the real program, and declare each fault
  it handles after a step took effect, with its case and the system's test.
- `differing-case`: the last real run found cases whose answers differ
  (`.truefit/<name>.differ.json`, with both answers): make the stand-in
  answer as the real program does.
- `nondeterministic-sim`: kept seeds whose two runs differed, each with the
  first line they differ at: make the simulation choose all from the seed.
- `failing-seed`: a kept seed that failed when last run: fix the bug it
  reaches.
- `unregistered-promise`: candidates not yet marked, and those marked a
  promise that no `[[promise]]` registers (`evidence`: `unmarked`,
  `unregistered`): mark each, register each promise.
- `unchecked-promise`, `unreached-promise`: a promise `truefit promise`
  reports so, with its report as `evidence`.
- `stale-model`: a model whose covered files (`covers`) changed since the
  `commit` its coverage record names, or whose `commit` this repo's history
  lacks: bring the model in line with the code, check it, and move `commit`
  forward. Outside a git repo it is not looked for, and `notes` says so.

The answer has the shape of `slimfit next --all`'s, so a factorium
`[idle]` tasks line files its tasks as issues, one open issue per kind:
`tasks = "slimfit --version && slimfit next --all && truefit next --all"`.
