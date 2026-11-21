# `truefit`

Keeps a system's tests and models true to the real system.

You (the agent) test a system through stand-ins of the programs it drives,
simulate it under seeded faults, and model its design. Each is only as good as
its tie to the real thing: a stand-in that answers as the real program does
not makes every test pass while the real system breaks. truefit holds those
ties, and tells you what to do where one is missing or stale.

Nothing is built yet. The commands to come, in order:

- `truefit contract`: each declared stand-in and its real program answer the
  same cases alike.
- `truefit next`: the next task, with its evidence and how to accept it.
- `truefit sim`: seeded runs of the system's own simulation; a failing seed
  reruns exactly.
- `truefit model`: the system's models, and its real runs checked as traces of
  them.
