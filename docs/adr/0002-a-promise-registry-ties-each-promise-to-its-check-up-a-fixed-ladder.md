# 2. A promise registry ties each promise to its check, up a fixed ladder

Status: accepted, 2026-11-21 (truefit#8, from the owner's question in the control session: how can truefit guide the AI to use model proofs where they are warranted?).

## Context

truefit holds contracts, simulations and models (ADR 0001), but nothing says where a model or a simulation is owed. A system's SPEC and ADRs make promises ("exactly once", "never", "at most one", "within N seconds"); nothing tied a promise to the check that keeps it, so one could sit with no check, or with a check that never exercises it, and nobody saw.

Precedent: Antithesis catalogs every assertion and counts one never reached as failing; FoundationDB passes a test ensemble only when every code probe was hit; Kiro links each requirement to a property and its test, and concedes some map to none. Extracting properties from prose in one step is unreliable (arXiv 2504.01294): annotate candidates first, then convert.

## Decision

1. **A registry in `truefit.toml`**: each `[[promise]]` has the invariant's one `name` (as the code, the simulation and the model use it), the sentence it `says`, its `source`, and its `check`s, or `kept = "<reason>"`. `truefit promise` reports each `checked`, `unreached`, `unchecked` or `kept`, and fails on the middle two.
2. **Registered but never exercised fails.** A `sim` check is reached only when a step of the last `truefit sim` named the invariant in its `checked`, in a state it is about (the simulation says which; truefit cannot judge a state); a `model` check only when the model's config checks an invariant of that name. `test` and `contract` are taken as declared: truefit links no system's code (ADR 0001) and cannot see a test reach a promise.
3. **A fixed ladder, in code**: test, contract, sim, model, from the cheapest. A promise about what runs at once or in an order the system does not choose (crash and restart, retry, lease, queue, a message lost or twice) needs both `sim` and `model`; a cheaper check, or `kept`, needs its reason in the registry. Its words mark it (or `concurrent = true`), so registering a concurrent promise as a test alone takes a reason, not an omission.
4. **Candidates in two steps**: truefit lists the sentences of `[promises] sources` that carry a promise word; the agent marks each `promise` or `not` (kept by the sentence's hash in `.truefit/promises.json`, so it is asked once, and again when the sentence changes), then registers each promise. truefit never decides from prose alone that a sentence is a promise.
5. `truefit next` gives `unregistered-promise`, `unchecked-promise` and `unreached-promise` tasks, so factorium's `[idle]` files them.

`[promises]` (the sources) is plural because TOML cannot hold a table `promise` beside the array `[[promise]]`.

## Consequences

- A sentence and its candidate are matched by text: a registered promise's `says` held in a sentence answers that candidate. Reworded docs ask again.
- Word lists find candidates and the concurrent kind; both miss some and catch some that are not. The marks and `why` are where the agent's judgment goes, written down.
- A `sim` check needs a simulation that prints `checked` per step; a simulation that does not reaches no promise.
