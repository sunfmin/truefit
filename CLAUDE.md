# truefit

A CLI that keeps a system's tests and models true to the real system: stand-ins answer as their real programs do (contracts), a simulation's failing seed reruns exactly, a model is checked against real runs (traces). Its users are AI agents; `truefit --help` is their manual (`src/manual.md`). Keep it true whenever behaviour changes.

What truefit is and is not: `docs/adr/0001-truefit-holds-the-ties-between-tests-models-and-the-real-system.md`.

## Checks

- `./gate.sh`: fmt, clippy, then `slimfit check` (the tests, measured against `.slimfit/baseline.json`). Run it before `factorium worker done`.
- slimfit's loop holds here as in any repo it is set up in (`slimfit --help`): a change that adds source lines carries its mutants record, picked by a second agent.

## Agent skills

### Issue tracker

Issues live in factorium's local tracker (`factorium issue … truefit`). See `docs/agents/issue-tracker.md`.

### Domain docs

Single-context. See `docs/agents/domain.md`.
