# The system under simulation: a queue delivers each message to a worker,
# which applies it, then acks it. A worker may crash (a fault the seed
# chooses) after applying a message and before acking it; the queue then
# delivers the message again. The seeded bug: the worker marks a message done
# only when it acks, so a crash before the ack applies the message twice.
#
# One clock (the step), one random source (seeded by the seed), and the
# invariant in one home: applied_once(). Each step names in `checked` the
# invariants it checked in a state they are about: applied-once where the
# message was delivered before (a check after a first delivery is vacuous).

function applied_once(   m) {
  for (m = 1; m <= N; m++) if (applied[m] > 1) return 0
  return 1
}

function state(   m, s) {
  s = "{\"applied\":["
  for (m = 1; m <= N; m++) s = s (m > 1 ? "," : "") applied[m] + 0
  return s "],\"next\":" head ",\"worker\":\"" worker "\"}"
}

# A restarted worker's name: from the restart count, or (made nondeterministic
# on purpose) from this process's id, which no seed chooses.
function name() { return (pid != "" && restarts ? "p" pid "-" : "w") restarts }

BEGIN {
  N = 3; head = 1; restarts = 0
  srand(seed + 0)
  worker = name()
  for (step = 1; head <= N; step++) {
    m = head; delivered[m]++
    if (!(m in done)) applied[m]++
    if (rand() < 0.35) {
      restarts++; worker = name(); event = "apply " m ", crash"
    } else {
      done[m] = 1; head++; event = "apply " m ", ack"
    }
    checked = delivered[m] > 1 ? "\"applied-once\"" : ""
    printf "{\"step\":%d,\"event\":\"%s\",\"state\":%s,\"checked\":[%s]}\n", step, event, state(), checked
    if (!applied_once()) {
      printf "{\"verdict\":\"fail\",\"invariant\":\"applied-once\",\"step\":%d}\n", step
      exit
    }
  }
  printf "{\"verdict\":\"pass\",\"steps\":%d}\n", step - 1
}
