# Failure scenarios — incident log

Symptoms only. No file names, no line numbers, no causes. Each one is a real
defect in the current code that you can reproduce today.

Treat each as a page at 3am: reproduce it, form a hypothesis, find the cause,
fix it, then write the check that would have caught it.

Some entries share a root cause. Some that look related don't. Finding out
which is the exercise.

Status: ⬜ open · 🔍 tracing · ✅ fixed + test

---

## A. The node stops existing

### ⬜ A1 — One read of a key that was never written, and the node is gone
```
POST localhost:3001/get {"key":"never-written"}
```
Connection drops. Every subsequent request to that node also drops — including
reads for keys you know are there, and writes from the leader. `ps` shows the
process is still alive. Only a restart brings it back.

### ⬜ A2 — The load generator kills the cluster in under a second
Start a clean cluster, run `stress.rs`. Within the first moments the leader
stops answering. Errors reported by the tool climb to 100%. The leader process
is still running.

### ⬜ A3 — A write that arrives a moment too early takes the leader down
Start the cluster through the orchestrator. Send a write to the leader in the
window between "the port is open" and "the orchestrator reports success".
The leader dies. Do it a second later and everything is fine.

### ⬜ A4 — A single malformed write kills a follower permanently
```
POST localhost:3001/set {"encrypted_payload":"garbage"}
```
That follower is finished. Note who can reach that port.

### ⬜ A5 — Asking for too many replicas takes the orchestrator down
```
POST localhost:4000/init {"replication_count": 65000}
```
The orchestrator dies. The cluster it had already started does not.

---

## B. The data quietly disagrees

### ⬜ B1 — A follower is down for an hour and every write still returns 200
Kill one follower. Keep writing to the leader. Every write succeeds. The
orchestrator still lists four healthy nodes. Nothing in any log mentions the
dead node. Reads against it are wrong for as long as it stays down.

### ⬜ B2 — Two followers disagree about whether a key still exists
Write a key with `expire: 30`. Around the 30-second mark, read it from each
follower in a loop. For a window, some say found and some say expired. Widen
the window by putting the nodes on hosts whose clocks differ.

### ⬜ B3 — Anyone can delete a key from a follower and the leader won't notice
```
POST localhost:3002/delete {"key":"hello"}
```
No signature, no token. The leader still has the key. Reads now depend on which
node answers. The leader will never re-send it.

### ⬜ B4 — A follower restarts and silently stops accepting replication
Kill one follower, start it again by hand on the same port. The leader keeps
sending it writes and keeps returning 200 to clients. That node never receives
another value, and nothing reports it.

---

## C. Everything hangs

### ⬜ C1 — One frozen follower stops all writes, cluster-wide
Freeze a follower rather than killing it — `kill -STOP` on its pid, so TCP
still accepts but nothing ever replies. Now write to the leader. The request
never returns. Neither does any other write. The leader is healthy, idle, and
completely stuck. `kill -CONT` and everything unblocks at once.

### ⬜ C2 — Reads stall behind a stuck write
While C1 is in progress, try to read from the leader. Note what happens, and
whether it depends on which key you ask for.

---

## D. Security

### ⬜ D1 — A stranger can take over a follower
Anyone able to reach a follower's port can hand it a key of their own choosing,
at any time, with no credential. After that, writes signed by the real leader
stop working and writes signed by the stranger start working.

### ⬜ D2 — A stranger can take over the leader
Same idea, against the leader, and the payload also carries the list of nodes
the leader replicates to. Consider where those writes end up.

### ⬜ D3 — The port the orchestrator picked wasn't yours
Start an unrelated service on 3002 that answers HTTP on `/`. Now call `/init`.
It reports success. Inspect what that service received, and what the cluster
thinks its third node is.

---

## E. Lifecycle and orchestration

### ⬜ E1 — `kill -9` the orchestrator and the cluster outlives it
Four processes keep running and keep holding their ports. Nothing supervises
them. Now try to start a fresh orchestrator and call `/init` again.

### ⬜ E2 — A failed `/init` leaves processes behind, then blocks the retry
Make `/init` fail partway — take one port before calling it, so one node can
never become ready. Read the error. Then look at what is running, and try
`/init` again.

### ⬜ E3 — `/init` succeeds against a cluster that isn't the one it started
Leave the processes from E1 running. Start a new orchestrator, call `/init`.
It reports four healthy nodes. Check which processes are actually serving, and
which keys they hold.

### ⬜ E4 — A node dies at 3am and the cluster reports full health
Kill any single node an hour after startup. Ask the orchestrator what the
cluster looks like. Ask the leader. Compare with `ps`.

---

## F. Performance

### ⬜ F1 — Throughput doesn't improve when you add cores
Run the load generator against a machine with spare CPU. Watch p99 climb with
concurrency while the CPU stays largely idle.

### ⬜ F2 — Every write pays a new TCP handshake per follower
Watch sockets on the leader during a sustained write load — `ss -s`, or count
`TIME_WAIT`. Compare the count to the number of writes. Then compare write
latency to the round-trip time of a single follower call.

### ⬜ F3 — Memory grows until the process is killed
Write unique keys with no TTL, continuously. Watch RSS. Find the thing that
stops it. (There isn't one — the question is what *should* stop it, and what
that costs you once the cluster has to agree.)

### ⬜ F4 — Expired keys hold memory long after they expire
Write many keys with `expire: 1`. Wait. Watch RSS, and time how long it takes
to drop. Then read one of them and watch RSS again.

---

## G. Known ceilings — not bugs, these are the reason for the roadmap

Don't "fix" these here; they're what Phases 9–15 are for.

### ⬜ G1 — The leader dies and nothing takes over
Kill the leader. Reads keep working and keep returning older data forever.
Writes fail. Nothing recovers without a human.

### ⬜ G2 — Everything is gone on restart
Restart any node. Ask it for a key it held a second ago.

### ⬜ G3 — A network partition splits the truth
Block traffic between the leader and two followers. Write to the leader. Read
from the isolated side. Heal the partition. Read again from both sides, and
decide which answer was supposed to be correct.

---

## How to work through this

1. Reproduce before you read any code. The repro is the evidence.
2. Write the hypothesis down first — you'll learn more from a wrong one than
   from jumping to the fix.
3. Fix the cause, not the symptom. Several of these are the same cause seen
   from different angles; if your fix is in one caller, check the siblings.
4. Every fix leaves one runnable check behind. A scenario without a test is a
   scenario that comes back.
