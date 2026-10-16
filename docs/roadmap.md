# Roadmap and design history

The original `plano_fbfast_final.md` was an implementation proposal, not an
accurate release checklist. Distribution evolved from manual copying to bundled
wheels and PyPI, and the author narrowed the release scope in
[decision 0018](decisions/0018-release-scope.md). The obsolete root plan has been
removed; its full text remains in Git history (`git log -- plano_fbfast_final.md`).

Current contracts live in the README, source/stubs, tests, and
[architecture decisions](decisions/). [Progress](progress.md) preserves dated
validation evidence; it is not a statement that every original milestone passed.

## Remaining work

- **Parallel analytical extraction:** opt-in partitioning of a restricted,
  deterministic single-table SELECT, or a structured export API. Resolve and
  quote identifiers through metadata; reject unsupported SQL rather than silently
  running serially. Use disjoint integer ranges, explicit inclusive bounds, and
  separate NULL handling; test empty/equal/extreme bounds, duplicates, gaps, and
  INT128 overflow. Do not use OFFSET or promise global ordering.
- **One snapshot for every partition:** reserve all requested pool slots
  atomically, including the leader in the connection budget. Share an active
  READ ONLY SNAPSHOT via the Firebird snapshot number; keep its leader alive
  until completion. Discover bounds in that snapshot. Cancel and clean up the
  whole group on failure; never return a partial result as complete. Prove
  serial/parallel equivalence, including multiplicity and NULLs, in one snapshot.
- **Broader performance evidence:** tuned equivalent-driver baselines, queue /
  prepare / execute / fetch / conversion / cleanup timings, RSS, WAN latency,
  BLOB inline/cache negotiation, batch sizing, and responsiveness under load.
  Keep raw artifacts and distinguish fixture tests from application validation.
- **Capability and failure coverage:** broader charset/capability matrices,
  network interruption and cleanup cases, and detailed duration metrics.
- **Compatibility extensions:** evaluate named parameters, DSNs, other server
  versions, free-threaded Python and stable-ABI wheels only with concrete demand
  and their own validation.

## Research gates

Optimize measured library bottlenecks before considering a custom wire protocol.
A wire prototype needs evidence of an official-client bottleneck, a bounded
hypothesis, and equivalent correctness. The original research gates were at
least 20% improvement in the affected workload and no more than 10% regression
in priority unaffected workloads, above measurement noise. They are experiment
criteria, not product performance promises.

Deferred ideas include fetch pipelining, batched BLOB transport, zero-copy text,
SIMD/PGO, and page/DB_KEY-based extraction. Each needs ownership, compatibility,
precision, cancellation, and reproducible performance evidence. Do not trade
away read-only semantics, encryption, exact values, cleanup, or consistent
snapshots for a benchmark result. Do not retry SQL already submitted implicitly.
