# TODO — updated 2026-10-09

**Context: this runs in a local test environment.** Process lifecycle, orphaned
children, port squatting and shutdown handling are parked — the box is yours,
`pkill -x` is a fine stop button, and nothing here is exposed. They come back
when something other than you starts these processes.

Score so far: **9 of 26 scenarios closed** (`FAILURES.md`).

---

## ✅ Done

### Section A — every panic path (2026-10-08)
`AppError` + `IntoResponse`, poison-safe locks, every request path returns a
`Result`. A5/A4/A3/A1 now answer 400/400/503/200 and the node keeps serving.

### Block 1 — the fan-out (2026-10-09)
C1, F2 and B1, closed by one shared `replicate()` helper used by both `/set`
and `/delete`:

```
frozen follower -> POST /set     0.009s  200
frozen follower -> POST /delete  0.001s  200    (was: hung forever)
```

Also fixed in passing: `Ok(_)` was counting a follower's 4xx/5xx as an ack, and
`needed` forgot that the leader is itself a cluster member.

**C2 answered:** reads never stalled behind a stuck write. The store lock is
dropped before the fan-out await. Remember why before Phase 10 touches locking.

---

## Today

### Block 2 — Make it measurable (Roadmap Phase 8)

Block 1 was the gate and it's open. You are in a test environment, so this is
the work that makes testing mean anything.

- [ ] Fix `stress.rs`. Re-measure first — its old error counts were section A's
      panics, not load.
- [ ] Flags: read/write ratio, key-space size, key skew.
- [ ] `/stats` on every node: hits, misses, hit rate, entries, expirations,
      **replication failures**, **replication deadline hits**. Plain
      `AtomicU64`; no metrics crate.
- [ ] Add the Raft fields now while they're free: role, term, commit index,
      last applied, log length. They stay zero until Phase 9.
- [ ] Switch `println!` to `tracing` (already a dependency, still unused). Stamp
      every line with node id. You will read thousands of these during Phase 10.

**Check:** state the hit rate and p99 as numbers, per node, from one command.

### Block 3 — Cheap correctness, independent of environment

- [ ] **B3** — `/delete` on a follower takes an unsigned body. Anyone who can
      reach the port deletes a key the leader still holds. Sign deletes exactly
      as sets are signed. (Small, and it removes a whole divergence source.)
- [ ] **B2, half of it** — stop running the expiry sweeper on followers. A
      follower should drop a key only when the leader says so, or lazily on
      read. One line, and it's the direction Phase 9 forces anyway.
- [ ] Delete the unused `active_follower_list` field, or make it the straggler
      tracker B5 needs. Don't leave it as decoration.

---

## Parked — test environment, revisit when that changes

Not fixed, not forgotten. Each has a trigger that un-parks it.

| | Scenario | Un-park when |
|---|---|---|
| **E1** | `kill -9` orchestrator orphans every node | anything but you starts these |
| **E2** | failed `/init` leaves processes behind | `/init` runs unattended |
| **E3** | `/init` succeeds against a stale cluster | same |
| **D3** | `/init` configures a stranger's service on a taken port | shared host or CI |
| — | SIGTERM handling / `POST /shutdown` | you stop using `pkill -x` |
| **D1/D2** | `/init_data` is unauthenticated | **anything leaves localhost** |

`kill_on_drop` never fires today — SIGTERM/SIGKILL don't unwind, so the handles
are never dropped. Ctrl-C only works because the shell signals the whole
process group. Harmless here, wrong everywhere else.

**D1/D2 is the one to watch.** It's cheap now and it's the kind of thing that
never gets added later. The moment this binds anything but `127.0.0.1`, it
jumps to the top.

---

## Known and deliberate

**B5 — quorum-ack strands a follower.** Today's change returns as soon as a
majority acks, which cancels the in-flight request to the slowest node:

```
freeze a follower -> write k3 -> 200
unfreeze, read k3:   3001 Key not found | 3002 Found | 3003 Found
```

Correct trade, incomplete system: nothing catches that follower up, so it
serves a miss for that key forever. The fix is not a patch here — it is the
replicated log (Phase 9) and `nextIndex`/`matchIndex` retry (Phase 11). Leave
it broken on purpose and let it motivate the log.

**Quorum isn't enforced either.** `replicate()` returns the ack count and both
callers ignore it, so a write that reached nobody still answers 200. Decide
whether `/set` should fail when the deadline passes below `needed` — that is a
real semantic choice, not an oversight, and Phase 11 will force it.

---

## Next, after today

1. **Phase 9 — the log.** Commands, append-only file, replay on startup.
   Expiry becomes a replicated entry instead of each node deciding alone — B2's
   real fix, and the end of B5.
2. **Phase 10 — election.** `TodaysPlan` item 3, finally reachable.
3. **Phase 16 — the deterministic harness.** Worth pulling in before Phase 10:
   seeded RNG, simulated clock, in-process network. Every Raft bug after that
   is a seed number instead of a story.
