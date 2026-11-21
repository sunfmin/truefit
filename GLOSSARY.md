# Glossary

- **Stand-in**: a program a system's tests run in place of a real one it drives (launchctl, tmux, an engine, 1Password's `op`, a store). Also called a fake.
- **Contract**: the cases a stand-in and its real program must answer alike. A stand-in without one is untested against reality.
- **Fault**: a failure a stand-in can answer (a refused edit, an unreachable network), named in its `faults` and held by a contract case whose real side fails the same way. A stand-in with none makes every test through it succeed.
- **Real run**: a contract's cases run against the real program, outside any gate. Its date is the contract's freshness.
- **Simulation**: the system's own code run step by step against stand-ins, its order of events and faults chosen from a **seed**; a failing seed reruns exactly.
- **Model**: a specification of the system's design (TLA+), checked by a model checker within bounds.
- **Trace**: what a real or simulated run recorded, checked as a behaviour of the model.
- **Coverage record**: which code each of a model's actions stands for (file and function), and the commit at which the model was last confirmed to describe it. Code changed since makes the model stale.
- **Invariant**: a property every state must hold; one home, used by the model, the simulation and the code's own checks.
- **Promise**: a sentence the system's SPEC, README or ADRs say of its behaviour ("never", "exactly once", "at most one", "within N seconds"), registered by its invariant's name and tied to the checks that keep it: test, contract, sim, model, cheapest first (the **ladder**). One a named check never reached, or none checks, fails; one about what runs at once or in an order the system does not choose needs a simulation and a model.
- **Candidate**: a sentence with a promise word, not yet marked a promise or not.
- **Tie**: any of the above that binds a test or a model to the real system.
