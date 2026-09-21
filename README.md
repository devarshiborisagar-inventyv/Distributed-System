# Mini Distributed In-Memory Cache — Learning Roadmap

## Goal

Build a distributed in-memory cache (think Memcached / Redis Cluster, not a
database) from scratch to learn:

-   Shared mutable state under concurrency
-   TTL / expiry
-   Eviction policies (LRU, LFU, random)
-   Sharding & consistent hashing
-   Replication for read scaling
-   Cache invalidation
-   Cache stampede / thundering herd
-   Hot keys
-   Node failure & rebalancing
-   Cluster membership (gossip)
-   Observability: hit rate, latency, memory

**Non-goals** (this is a cache, data loss is acceptable):
durability, WAL, crash recovery, consensus/Raft, quorum writes, transactions.

------------------------------------------------------------------------

# Tech Stack

## Language

**Rust** — memory safe, great async, real distributed systems use it.

## Libraries

  Purpose              Library
  -------------------- --------------------
  Async Runtime        tokio
  HTTP Server          axum
  HTTP Client          reqwest
  Serialization        serde + serde_json
  Time                 chrono
  Logging              tracing
  CLI                  clap
  Hashing              std DefaultHasher, then xxhash/ahash
  Metrics              plain counters, then /metrics text
  Config               toml

Current layout (keep it flat until it hurts):

``` text
src/
├── cache.rs                 # shared: Data, SetReq/KeyReq/KeyRes, expiry sweeper
├── main.rs                  # leader node (:3000)
├── bin/follower.rs          # replica node (:3001-3003)
├── bin/stress.rs            # load generator
└── handlers/
    ├── handlers.rs          # follower handlers
    └── leader_handlers.rs   # leader handlers (+ fan-out)
```

The crate is still named `mini-db`; rename later, it changes nothing.

------------------------------------------------------------------------

# Roadmap

Status: ✅ completed · ⚠️ revision needed · ⬜ open

Order changed from the original plan for two reasons: TTL landed early (moved
up to Phase 4), and everything from Phase 7 on is *measured*, so the broken
load generator and the hit/miss counters were pulled forward into Phase 6 —
you cannot evaluate eviction, stampede, or hot keys without them.

------------------------------------------------------------------------

## ✅ Phase 0 — Rust + single-node store

-   Axum server, `GET /`
-   `Arc<Mutex<HashMap>>` shared state
-   SET / GET / DELETE

**Checkpoint met:** in-memory KV store on one node.

------------------------------------------------------------------------

## ✅ Phase 1 — Multiple nodes

Leader on 3000, followers on 3001/3002/3003 via `run.sh`.

**Checkpoint met:** several nodes running side by side.

------------------------------------------------------------------------

## ✅ Phase 2 — Replication

Writes go to the leader; leader forwards `/set` and `/delete` to every
follower. Verified end-to-end: set on leader → read on follower → delete on
leader → miss on follower.

**Checkpoint met:** a write on the leader is visible on all followers.

------------------------------------------------------------------------

## ✅ Phase 3 — Shared code extracted

`Data`, `SetReq`, `KeyReq`, `KeyRes`, `Store` and the expiry sweeper live in
`src/cache.rs` and are used by both leader and follower. `GetReq`/`DeleteReq`
collapsed into `KeyReq`, `GetRes`/`DeleteRes` into `KeyRes`.

**Checkpoint met:** one definition of the wire format, one definition of a
cached value.

------------------------------------------------------------------------

## ⚠️ Phase 4 — TTL / expiry

Built:

-   `SET {key, value, expire}` — server stamps `created_at`, client no longer
    sends it
-   `Data::is_expired()`, correct on ttl=0 (never expires) and on clock skew
-   Lazy expiry: GET filters expired entries and reports a miss
-   Active expiry: `start_passive_cleaner` sweeps every 10s on both roles
-   Unit test covers all four expiry cases

**Revision needed:**

1.  **Follower restarts the TTL clock.** The leader forwards the raw `SetReq`,
    so the follower stamps its own `created_at: Utc::now()`. Leader and
    follower expire at different instants — milliseconds today, but seconds or
    more once a follower is slow, retried, or reconnecting after a partition.
    Fix: replicate the resolved `Data` (with the leader's `created_at`), not
    the client's `SetReq`.
2.  **Lazy expiry doesn't reclaim.** `get_data` takes a read lock and only
    *filters* the expired entry; it never removes it. Memory is held until the
    10s sweeper runs. Fix when you touch the lock strategy in Phase 10.
3.  **`expire` units are undocumented at the API boundary.** It's seconds, and
    only a doc comment in `cache.rs` says so. Name it `expire_secs`, or
    document it in the README's API section.
4.  **No way to set a key without a TTL from a client that doesn't care.**
    `expire` is a required field (missing → 422). Add
    `#[serde(default)]` so `0` = no expiry is the default.

------------------------------------------------------------------------

## ⚠️ Phase 5 — Replication hardening

Built:

-   `join_all` fan-out, so replication costs the slowest follower, not the sum
-   DELETE fans out too (leader and followers no longer diverge on delete)

**Revision needed:**

1.  **No timeout on any outbound call.** One hung follower stalls every write
    on the leader indefinitely. This is the single most important fix in the
    file — every later phase that measures latency is meaningless until it
    lands. Fix: `.timeout(Duration::from_millis(200))`.
2.  **A new `reqwest::Client` per follower per request.** Each one builds a
    fresh connection pool, so keep-alive never kicks in and every write pays a
    TCP handshake per follower. Fix: build one `Client` at startup, put it in
    `AppState`, clone it (it's an `Arc` inside).
3.  **`join_all`'s result is dropped.** Every replication failure is silently
    discarded — a follower can be down for an hour and the leader never says
    a word. Fix: count failures, log them, expose the count in Phase 6.

**Still open in this phase:**

-   Followers accept writes from anyone. `/set` and `/delete` on a follower
    should only accept leader traffic (shared secret header is enough).
-   Decide sync vs async per write: wait for followers, or `tokio::spawn` the
    fan-out and return immediately. Right now it's always sync.

------------------------------------------------------------------------

## ⬜ Phase 6 — Make it measurable (pulled forward)

Nothing after this phase can be evaluated without it.

-   **Fix `stress.rs`** — it still posts `{"key","value"}` with no `expire`
    and gets a 422 on every write. Confirmed: every "error" it reports today
    is its own bad request shape, not server load.
-   Give it a read/write ratio flag and a key-skew flag (needed in Phase 9
    and 16).
-   Add `/stats`: hits, misses, hit rate, entries, evictions, expirations,
    replication failures.
-   Counters as `AtomicU64` in `AppState`; no metrics crate yet.

**Checkpoint:** you can state the hit rate and p99 as numbers, per node.

------------------------------------------------------------------------

## ⬜ Phase 7 — Bounded memory + eviction

The defining feature of a cache: it is allowed to forget.

-   Add a `max_entries` cap
-   Implement eviction, simplest first:
    1.  Random victim
    2.  LRU (`HashMap` + intrusive order, or an ordered map)
    3.  Sampled LRU — pick K at random, evict the oldest
-   Report evictions in `/stats`

Learn: why Redis samples instead of keeping a perfect LRU list.

**Checkpoint:** memory stays flat under an unbounded write load.

------------------------------------------------------------------------

## ⬜ Phase 8 — Cache-aside pattern

-   Add a fake slow "origin" (sleep 200ms) behind the cache
-   GET → miss → fetch from origin → populate → return
-   Measure hit rate and p99 with and without the cache

Learn: read-through vs cache-aside, and where the win actually comes from.

**Checkpoint:** the cache's value is a measured number, not an assumption.

------------------------------------------------------------------------

## ⬜ Phase 9 — Cache stampede

-   Hammer one cold key with 500 concurrent readers
-   Watch the origin get 500 identical requests
-   Fix with single-flight: one in-flight fetch per key, the rest wait

Learn: thundering herd, request coalescing, why TTL jitter exists.

**Checkpoint:** 500 concurrent misses cause exactly one origin call.

------------------------------------------------------------------------

## ⬜ Phase 10 — Concurrency & lock contention (moved up from 16)

The last single-node concern before going distributed — and where Phase 4's
lazy-expiry reclaim gets fixed for free.

-   Profile the single `Mutex<HashMap>` under `stress.rs`
-   Replace with sharded locks (`Vec<Mutex<HashMap>>`, index by key hash)
-   Compare against `RwLock` and `dashmap`
-   Make GET able to remove an expired entry it just found

Learn: where the lock actually was, measured not guessed.

**Checkpoint:** throughput scales with cores.

------------------------------------------------------------------------

## ⬜ Phase 11 — Sharding by key

Stop replicating everything. Split the keyspace instead.

-   Client (or a router) picks a node: `hash(key) % node_count`
-   Each node owns a disjoint slice of the keyspace
-   Capacity now scales with node count

**Checkpoint:** three nodes hold ~1/3 of the keys each.

------------------------------------------------------------------------

## ⬜ Phase 12 — Consistent hashing

-   Remove a node from `hash % N` and watch nearly every key move
-   Replace with a hash ring + virtual nodes
-   Measure how many keys move when a node joins or leaves

**Checkpoint:** adding a 4th node relocates ~1/4 of keys, not all of them.

------------------------------------------------------------------------

## ⬜ Phase 13 — Node failure

-   Kill a shard owner
-   Requests for its keys must miss, not hang or 500 (Phase 5's timeouts are
    the prerequisite)
-   Optional: circuit breaker — mark a node dead after K failures

Learn: a cache miss is a valid answer; a hang is not.

**Checkpoint:** killing a node degrades hit rate and nothing else.

------------------------------------------------------------------------

## ⬜ Phase 14 — Replicas per shard

-   Each shard gets N replicas (start with 2)
-   Reads may hit any replica, writes go to the shard primary
-   Observe reading your own write failing

Learn: read scaling vs staleness, read-your-writes.

**Checkpoint:** one replica dies and reads still succeed.

------------------------------------------------------------------------

## ⬜ Phase 15 — Invalidation

-   DELETE / PURGE must reach every replica of the shard
-   Deliberately drop one invalidation and watch a stale read
-   Handle the TTL-skew case from Phase 4 under a real partition

Learn: cache invalidation is the hard part, empirically.

**Checkpoint:** you can explain exactly how a given stale entry happened.

------------------------------------------------------------------------

## ⬜ Phase 16 — Hot keys

-   Skew the load: 90% of traffic on 1% of keys
-   Watch one node saturate while the others idle
-   Mitigate: client-side mini-cache, or replicate hot keys everywhere

Learn: why sharding alone does not balance load.

**Checkpoint:** hot-key traffic no longer pins a single node.

------------------------------------------------------------------------

## ⬜ Phase 17 — Membership & gossip

-   Nodes heartbeat each other instead of reading a hardcoded
    `followers_list` in `main.rs`
-   Failure detection by missed heartbeats
-   New node joins, ring updates, keys rebalance

Learn: gossip, SWIM basics, failure detector tradeoffs.

**Checkpoint:** the cluster list is discovered, not hardcoded.

------------------------------------------------------------------------

## ⬜ Phase 18 — Full metrics

Grow Phase 6's `/stats` into a real `/metrics`:

-   p50 / p99 latency histograms
-   requests/sec by op
-   replication lag per follower
-   per-node status, uptime
-   approximate memory, not just entry count

------------------------------------------------------------------------

## ⬜ Phase 19 — Dashboard

Display the ring, per-node hit rate, hot keys, node health.

Controls: kill node, add node, purge key, flush node, set TTL, change
eviction policy.

------------------------------------------------------------------------

# Bonus Challenges

-   Byte-based memory limits (not entry counts)
-   TTL jitter / early recompute
-   Write-through and write-behind modes
-   Negative caching (cache the miss)
-   Compression for large values
-   Binary protocol, or speak real RESP so `redis-cli` connects
-   Pipelining, batched MGET
-   gRPC, TLS

------------------------------------------------------------------------

# Concepts You'll Learn

Rust ownership · async / tokio · shared state & lock contention · TTL ·
eviction policies · cache-aside · stampede & single-flight · sharding ·
consistent hashing · replication · staleness · invalidation · hot keys ·
failure detection · gossip · observability

------------------------------------------------------------------------

# Recommended Reading

  After Phase   Read
  ------------- -------------------------------------------------------
  0             The Rust Programming Language (Ch. 1–10)
  3             Tokio Tutorial
  7             Redis eviction policies docs; TinyLFU / W-TinyLFU paper
  9             "Caching at Scale" / single-flight write-ups
  10            Rust Atomics and Locks
  12            Consistent Hashing (Karger et al.); Amazon Dynamo §4
  14            DDIA — Chapter 5 (replication)
  16            Facebook Memcache paper ("Scaling Memcache at Facebook")
  17            SWIM paper

------------------------------------------------------------------------

# Estimated Timeline

-   **Now:** Phase 4 + 5 revisions (timeouts, shared client, TTL stamping)
-   Week 1: Phase 6 (fix stress.rs, add /stats)
-   Week 2: Phases 7–9 (eviction, cache-aside, stampede)
-   Week 3: Phase 10 (lock contention)
-   Weeks 4–5: Phases 11–13 (sharding, ring, failure)
-   Weeks 6–7: Phases 14–16 (replicas, invalidation, hot keys)
-   Weeks 8–9: Phases 17–19 (gossip, metrics, dashboard)

------------------------------------------------------------------------

By the end you'll have built a miniature distributed cache demonstrating the
ideas behind Memcached, Redis Cluster, and Facebook's memcache tier.
