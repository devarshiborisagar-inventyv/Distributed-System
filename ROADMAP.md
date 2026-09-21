# Roadmap — from in-memory cache to Raft-replicated KV store

**Supersedes the roadmap in `README.md` from Phase 6 onward.** Phases 0–5 there
still describe what was built. Everything after it was planned for a *cache*
whose stated non-goals were "durability, WAL, crash recovery, consensus/Raft,
quorum writes". That is now the goal, so the second half of that roadmap is
dead and is replaced by this file.

Scope: single-group Raft, low-to-medium scale, learning/research simulator.
Correctness you can *demonstrate* beats throughput you can quote.

---

## The pivot, stated plainly

A cache may forget. A replicated state machine may not.

Every node in a Raft group must apply the same commands in the same order and
end in the same state. That single rule invalidates three things currently in
the code or the old plan:

1. **Independent expiry.** `start_passive_cleaner` runs on the leader *and* on
   every follower, each on its own clock. Two nodes delete the same key at
   different instants — divergence. Under Raft, expiry is a decision the leader
   makes and *replicates as a log entry*. Followers never expire anything on
   their own. (This is exactly why Redis replicas don't expire keys themselves;
   the primary ships an explicit `DEL`.)
2. **Eviction (old Phase 7).** Node-local LRU means node A drops `k` and node B
   keeps it. Either eviction is replicated too, or the replicated keyspace is
   unbounded and eviction only applies to a separate, non-replicated cache
   layer. Decide which — don't build LRU into the state machine by reflex.
3. **Gossip membership (old Phase 17).** Raft carries its own membership and
   its own failure detector (missed heartbeats). Building SWIM first means
   deleting it later. Cut it.

Sharding and consistent hashing (old Phases 11–12) aren't wrong, they're just
*after* Raft: they become multi-Raft (one group per shard), Phase 17 here.

---

## Decisions to make before Phase 9

| Decision | Options | Recommendation |
|---|---|---|
| Write Raft yourself or use `openraft`? | hand-rolled / `openraft` | **Hand-rolled.** The learning *is* the project. Note `openraft` as the answer if this ever needs to actually work. |
| Cache semantics or DB semantics? | keep TTL+eviction / drop them | **Keep TTL, drop eviction** from the replicated keyspace. TTL-as-a-log-entry is a good Raft exercise; LRU-as-a-log-entry is just noise. |
| Keep Ed25519 per-message signing? | keep / shared-secret header / drop | **Downgrade to a shared secret** for now. A signature per `AppendEntries` is a real cost at heartbeat rates, and it is orthogonal to consensus. Revisit as a bonus. |
| Storage engine for the log | append-only file / `sled` / `redb` | **Append-only file** you write. Length-prefixed bincode/JSON + `fsync`. It's ~150 lines and it teaches the durability contract. |

---

## Phase 6 — Make it run at all

**It does not compile today, and replication has never worked.** Nothing below
this line is worth starting until this phase is green. Each item is a real
defect confirmed by reading the code, not a style note.

- [ ] **Compile error:** `handlers.rs:91` calls `.write().await` on a
      `std::sync::RwLock`. Either drop the `.await` or switch to
      `tokio::sync::RwLock`.
- [ ] **Compile error:** `bin/follower.rs:15` constructs `RwLock::new(...)`
      with no `RwLock` in scope.
- [ ] **Panic on every cache miss.** `handlers.rs:46` and
      `leader_handlers.rs:65` both do `map.get(&key).unwrap()`. A GET for a key
      that isn't there panics — *while holding the `Mutex`*. The lock is then
      poisoned, so every later `.lock().unwrap()` on that node panics too. One
      miss permanently kills the node. Fix both call sites at once by putting a
      single `get_live(&store, key) -> Option<Data>` in `cache.rs` that removes
      the entry if expired and returns `None` on miss. That also closes the
      "lazy expiry doesn't reclaim" item from README Phase 4.
- [ ] **Replication is 100% dead.** A follower starts with
      `verifying_key: ""`, so `string_to_verifying_key("")` fails the 32-byte
      check and `set_data` unwraps on `Err` — every replicated write panics.
      The `save_key` handler that would fix it **is not routed on any node**.
      Either route it, or default the follower to the public key that matches
      the hardcoded `SIGNING_KEY` (verified: it is
      `1Ps9YkDqB5j876JgwV0M5K1CXk9qKUtFw14c4NXnoUc`, the value already sitting
      commented out at `handlers.rs:9`).
- [ ] **No timeout on any outbound call** (`leader_handlers.rs:46`, `:94`). One
      hung follower stalls every write forever. Add
      `.timeout(Duration::from_millis(200))`.
- [ ] **A fresh `reqwest::Client` per follower per request.** Build one at
      startup, hold it in `AppState`, clone it — it's an `Arc` inside.
      Otherwise keep-alive never engages and each write pays a TCP handshake
      per follower.
- [ ] **`join_all`'s result is dropped.** Count and log the failures.
- [ ] **There are zero tests in the repo.** README claims "Unit test covers all
      four expiry cases" — it does not exist. Write it. This matters more than
      it looks: Raft bugs are invisible without tests, and you are about to
      write Raft.

**Checkpoint:** `cargo build` clean; set on leader → get on all three
followers; GET a missing key returns a miss and the node stays alive
afterward; one follower killed and writes still return in <300ms.

---

## Phase 7 — One binary, role at runtime

Raft nodes change role while running. A design where "leader" and "follower"
are two different modules chosen at compile time cannot express *"follower 3002
becomes leader at 3am"*. This is `TodaysPlan` items 4 and 5, and it is a
prerequisite for election, not a tidy-up.

- [ ] Collapse `handlers/handlers.rs` and `handlers/leader_handlers.rs` into
      one handler set. There is one `AppState`, one `set_data`, one `get_data`.
- [ ] Role becomes runtime state: `enum Role { Follower, Candidate, Leader }`
      inside the state, not a module choice.
- [ ] Handlers branch on role. A write to a non-leader responds "not leader,
      leader is X" (307 redirect or a JSON hint) rather than accepting it.
- [ ] Config from a TOML file + `clap`: node id, listen address, peer list.
      Delete the hardcoded `followers_list` in `main.rs:16`.
- [ ] `run.sh` starts N copies of the *same* binary with different configs.
- [ ] Delete `bin/follower.rs` and the empty `src/orchestrator.rs`.

**Checkpoint:** one binary, four configs, cluster behaves exactly as before.

---

## Phase 8 — Make it measurable

You cannot debug consensus by reading logs. Build the instruments before the
engine.

- [ ] Fix `bin/stress.rs`. Its errors today are the Phase 6 GET panic, not load
      — re-measure once Phase 6 lands before changing anything else in it.
- [ ] Add flags: read/write ratio, key-space size, key skew (Zipfian).
- [ ] `/stats` endpoint: hits, misses, hit rate, entry count, expirations,
      replication failures. Plain `AtomicU64` in state; no metrics crate.
- [ ] Add the Raft fields now so they're free later: current term, role,
      commit index, last applied, log length.
- [ ] Structured logging via `tracing` (the dep is already in `Cargo.toml` and
      unused — the code uses `println!`). Every node stamps every line with
      node id and term. You will read thousands of these lines.

**Checkpoint:** you can state hit rate and p99 as numbers, per node.

---

## Phase 9 — The log (the cache→DB turn)

This is where it stops being a cache.

- [ ] `enum Command { Set { key, value, expire }, Delete { key }, Expire { key }, Noop }`
- [ ] `struct LogEntry { term: u64, index: u64, command: Command }`
- [ ] Append-only file per node: length-prefix each entry, `fsync` on append,
      replay the whole file on startup.
- [ ] Persist `current_term` and `voted_for` durably and separately. **Election
      needs only these two to be durable** — the full log can stay in memory
      until Phase 11 if you want elections working sooner.
- [ ] Split the code in two conceptually: the **log** (what was agreed) and the
      **state machine** (the `HashMap`, built by applying committed entries).
      Writes no longer touch the map directly; they append, commit, then apply.
- [ ] Make expiry a `Command`. The leader's sweeper stops deleting and starts
      *proposing* `Expire{key}`. Followers delete only when they apply that
      entry. Delete the follower-side sweeper.
- [ ] Test: kill a node mid-write, restart, prove the log replays to the same
      state.

**Checkpoint:** a node restarts and comes back with its data.

---

## Phase 10 — Raft I: leader election

`TodaysPlan` item 3. Read the Raft paper §5.2 first, Figure 2 is the spec —
implement it literally, do not improvise.

- [ ] Randomized election timeout (150–300ms, jittered per node). The jitter is
      what prevents split votes; without it you will see infinite elections.
- [ ] `RequestVote` RPC: term, candidateId, lastLogIndex, lastLogTerm.
- [ ] Voting rules: one vote per term, persisted before replying. The
      up-to-date check (§5.4.1) is the part everyone gets wrong — a node must
      refuse to vote for a candidate whose log is behind its own.
- [ ] Any node seeing a higher term immediately steps down to follower.
- [ ] Empty `AppendEntries` as heartbeat; receiving one resets the timeout.
- [ ] New leader appends a no-op entry for its term (§8).

**Checkpoint:** kill the leader, a new one is elected in under a second, and
the cluster never has two leaders in the same term.

---

## Phase 11 — Raft II: log replication

- [ ] `AppendEntries` with `prevLogIndex`/`prevLogTerm` consistency check.
- [ ] `nextIndex[]` and `matchIndex[]` per follower on the leader.
- [ ] Rejection → decrement `nextIndex` and retry until the logs converge.
- [ ] A conflicting follower entry is **truncated**, not merged.
- [ ] Commit when a majority has `matchIndex >= N` **and** the entry is from
      the current term (§5.4.2 — the subtle one; skipping it loses data).
- [ ] Apply committed entries to the state machine in index order.
- [ ] Client write blocks until its entry commits, then returns.

**Checkpoint:** write to the leader, kill it, the new leader still has the
write, and all survivors' logs are byte-identical.

---

## Phase 12 — Raft III: safety under adversity

Where the learning actually happens. Deliberately break it.

- [ ] Partition the leader from the majority. It must stop committing and step
      down; the majority elects a new leader.
- [ ] Heal the partition. The stale leader must discover a higher term and
      truncate its uncommitted tail.
- [ ] Reject duplicate/stale RPCs by term.
- [ ] Client retry safety: a retried write must not apply twice. Add a client
      id + sequence number and dedupe in the state machine (§8).
- [ ] Write the property test: for any sequence of kills, partitions and
      writes, no two nodes ever apply different commands at the same index.

**Checkpoint:** you can narrate exactly why a given entry was or wasn't
committed.

---

## Phase 13 — Reads

Reads are not free in Raft, and the naive version is silently wrong.

- [ ] Show the bug first: read from a partitioned stale leader, get stale data.
- [ ] ReadIndex: leader confirms leadership with a heartbeat round before
      serving a read.
- [ ] Leader leases as the faster, clock-dependent alternative — implement and
      state the assumption it makes about clock drift.
- [ ] Offer explicit stale reads from followers as an opt-in flag, and measure
      how stale they get.

**Checkpoint:** three read modes, each with a measured latency and a stated
consistency guarantee.

---

## Phase 14 — Snapshots & compaction

The log grows forever until you stop it.

- [ ] Snapshot the state machine at index N; truncate the log before N.
- [ ] `InstallSnapshot` RPC for a follower that has fallen too far behind.
- [ ] A node restarting from snapshot + log tail reaches the same state.

**Checkpoint:** run the stress test for an hour; disk stays bounded.

---

## Phase 15 — Membership changes

- [ ] Single-server add/remove (the safe simplification over joint consensus).
- [ ] New node catches up as a non-voting learner before it may vote.
- [ ] Prove the unsafe version is unsafe: change membership in one step and
      construct the two-majorities split.

**Checkpoint:** grow 3→5 nodes with no downtime and no lost writes.

---

## Phase 16 — The simulator

This is the stated goal of the project, so make it a real artifact.

- [ ] Deterministic test harness: seeded RNG, simulated clock, in-process
      network. No real sockets, no real sleeps.
- [ ] Injectable faults: drop, delay, reorder, duplicate, partition, crash,
      restart, clock skew.
- [ ] Run thousands of seeds in CI; a failing seed replays byte-identically.
- [ ] Linearizability checker over the recorded history.

**Checkpoint:** a Raft bug is a seed number you can re-run, not a story.

---

## Phase 17 — Multi-Raft (scale out)

Only now does sharding mean anything.

- [ ] Hash the keyspace into ranges; one Raft group per range.
- [ ] Consistent hashing / ring for range→group mapping.
- [ ] A router that sends each key to its group's leader.
- [ ] Explicitly declare: no cross-shard transactions. That's a different
      project (Percolator / 2PC over Raft) — note it and stop.

---

## Phase 18 — Observability & dashboard

- [ ] p50/p99 histograms, per-op throughput, replication lag per follower.
- [ ] Raft-specific: term changes over time, election count, commit lag,
      apply lag, log size, snapshot age.
- [ ] Dashboard showing the ring, per-node role and term, live log indices.
- [ ] Controls: kill node, partition node, add node, force election.

---

## Reading, in the order you'll need it

| Before | Read |
|---|---|
| Phase 9 | Raft paper §5.1–5.3 (in-order, twice) |
| Phase 10 | Raft paper Figure 2 — treat as a spec, not a diagram |
| Phase 11 | Raft paper §5.4 (safety) |
| Phase 12 | *Students' Guide to Raft* (Jon Gjengset) |
| Phase 13 | Raft thesis §6.4 (read-only queries) |
| Phase 14 | Raft thesis §5 (compaction) |
| Phase 15 | Raft thesis §4 (membership) |
| Phase 16 | FoundationDB simulation talk; Jepsen write-ups |
| Phase 17 | TiKV multi-raft design; Spanner §2 |

---

## Honest sequencing

- Phases 6–8 are cleanup and instruments. They are not optional and they are
  not the fun part. Everything after them is unmeasurable without them.
- Phases 9–12 are the actual project. Budget most of your time here.
- Phase 16 is what makes it *research* rather than a toy. Consider pulling it
  earlier — a deterministic harness built before Phase 10 pays for itself
  within a week, because every Raft bug after that is reproducible instead of
  a heisenbug.
- Phases 17–18 are optional polish. A correct 3-node group with a
  linearizability checker is a stronger result than a sharded cluster you
  can't prove anything about.
