---- MODULE Counters ----
\* Five interchangeable workers, each counting from 0 to 9. Up to SYMMETRY,
\* a state is the multiset of their counts: C(14,5) = 2002 distinct states.
EXTENDS Naturals, TLC
CONSTANT Workers
VARIABLE count

Init == count = [w \in Workers |-> 0]
Next == \E w \in Workers : count[w] < 9 /\ count' = [count EXCEPT ![w] = @ + 1]
Spec == Init /\ [][Next]_count
Perms == Permutations(Workers)
====
