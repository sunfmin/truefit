# 1. truefit holds the ties between tests, models and the real system

Status: accepted, 2026-11-21 (the owner, in the control session).

## Context

In guild, every test passed while a soft stop let Claude Code run its next tool call. The `claude` stand-in honoured the stop answer the way Guild assumed; the real engine does not. Only a live run found it.

The owner's repos each tie stand-ins to real programs by hand, each its own way, most run by hand: factorium's `live_orca` test, factum's store conformance suite (memory and sqlite), slimfit's `examples/standin` beside `e2e.sh`, sesame's `fake_op` (1Password CLI 2.x), guild's fakes. factorium (one clock per scenario world) and factum (`Clock`, `Store`, `Engine::tick`) already step their own code; guild asks for a model tied by traces and a seeded simulation.

The owner (2026-11-21), choosing a tool over a skill: 「工具给 AI 提供的帮助就是 skill，同时在工具中可以做一些固定性的操作和行为，所以我更趋向于做一个工具来解决这一类的问题」.

## Decision

1. truefit is one CLI, like slimfit: its manual (`truefit --help`) guides the agent through each judgment; the fixed steps (run a contract against both sides and compare, rerun a seed, check a trace against a model, tell what is stale) are truefit's own code. `truefit next` gives tasks, which factorium's `[idle]` files as issues.
2. Three ties, built in this order: **contracts** (stand-in against real program), then **simulation** (seeds: run, rerun, keep failing ones), then **models** (TLA+, traces of real runs).
3. truefit drives a system only through commands the system declares in `truefit.toml`; it links no code of the systems it checks. factum underlies factorium, and factorium builds truefit: a link would make a loop.
4. First users, each tie's acceptance proven on one: contracts on factorium and sesame; simulation on factum, then factorium, then guild; models on factum's exactly-once and factorium's heavy queue, then guild.
5. truefit itself is kept small with slimfit, from its first commit.

## Consequences

- A tie truefit cannot run (the real program needs an account, a login, a device) is reported as not run, with what it needs; never counted as passing.
- truefit does not write a system's simulation or model; the manual says how, and truefit runs and checks what the system declares.
