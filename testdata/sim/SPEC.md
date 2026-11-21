# The queue

The queue delivers each message to a worker, which applies it, then acks it.
A worker may crash after it applies a message and before it acks it.

- A message is never applied twice, however often it is delivered.
- A crashed worker's message is never lost: the queue delivers it again.
- Only the messages 1 to N are ever in the queue's head.
- Every run of the simulation is chosen from its seed alone.

The worker here is only a sketch, in awk.
