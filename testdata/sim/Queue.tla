---------------------------- MODULE Queue ----------------------------
(* The queue's design, as queue.awk means it: the queue delivers its head  *)
(* message to the worker, which applies it, marks it done, then acks it.   *)
(* A worker may crash after applying a message and before acking it; the   *)
(* queue then delivers the message again, and the worker, finding it done, *)
(* does not apply it twice.                                                *)
EXTENDS Naturals

CONSTANT N            \* messages in the queue

VARIABLES
    applied,          \* applied[m]: how many times message m was applied
    head,             \* the message the queue delivers next (N + 1: none)
    done              \* the messages the worker marked done

vars == <<applied, head, done>>

Init == /\ applied = [m \in 1..N |-> 0]
        /\ head = 1
        /\ done = {}

\* The worker applies the head message unless it is done, and marks it done.
Apply == /\ applied' = IF head \in done THEN applied
                       ELSE [applied EXCEPT ![head] = @ + 1]
         /\ done' = done \cup {head}

\* queue.awk: "apply m, ack": applied, then acked; the queue moves on.
ApplyAck == /\ head <= N
            /\ Apply
            /\ head' = head + 1

\* queue.awk: "apply m, crash": applied, the worker crashes before the ack;
\* the queue delivers the same message again.
ApplyCrash == /\ head <= N
              /\ Apply
              /\ UNCHANGED head

Next == ApplyAck \/ ApplyCrash

Spec == Init /\ [][Next]_vars

\* queue.awk's applied_once(): no message is applied twice.
AppliedOnce == \A m \in 1..N : applied[m] <= 1

TypeOK == /\ applied \in [1..N -> Nat]
          /\ head \in 1..N + 1
          /\ done \subseteq 1..N
=============================================================================
