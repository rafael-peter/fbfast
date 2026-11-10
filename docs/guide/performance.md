# Performance and latency

On a remote link (VPN, Tailscale, Cloudflare Tunnel, another region) the cost
of a query is mostly **round trips × latency**, not CPU. This guide explains
where the round trips go, what fbfast 0.3.0 does about them, and what to tune
on the client and on the server.

Measure your own case first:

```python
config = fbfast.FirebirdConfig(diagnostics=True)
with fbfast.Pool(config) as pool:
    pool.fetch_all("SELECT ID FROM ORDERS WHERE STATUS = ?", ["OPEN"])
    for record in pool.diagnostics():
        print(record["roundtrips"], [(p["phase"], p["roundtrips"]) for p in record["phases"]])
    print(pool.session_info())  # inline_blobs, wire_compressed, protocol_version...
```

Reading the counters is local to the client (no extra round trip).
`pool.stats()` keeps cumulative totals (`wire_roundtrips`, `wire_sent_bytes`,
`wire_received_bytes`, `transaction_restarts`, `keepalive_pings`, ...).

## Round trips per query

| Step | 0.2.1 | 0.3.0 |
|---|---|---|
| ping after `health_check_after` idle | 1, before the query | 0: done in the background |
| start transaction | 1 | 0 if the previous query left one ready, else 1 |
| prepare | 1 on first use per connection | same (statements are cached) |
| execute + first batch | 1 | 1 |
| close cursor | 0 (piggybacks) | 0 |
| rollback + `ALTER SESSION RESET` | 2, before the reply | after the reply (1, and it starts the next transaction) |
| **critical path, cached statement** | **4 (5–6 cold or idle)** | **1 (2 after 2 s idle)** |

The reset that clears session state (context variables, temporary tables,
role, time zone) still runs after every query, and every query still gets its
own transaction. What changed is *when*: after the reply, while the connection
stays reserved, and as one statement (`ALTER SESSION RESET` inside the
transaction rolls it back, resets the session and starts the next one).

`ready_transaction_timeout` (default 2 s) rolls an unused ready transaction
back. While it exists it holds the server's oldest snapshot (OST), which can
delay garbage collection of newer record versions; it does not hold the
oldest active transaction. Set it to `None` to give up the saved round trip.

Trade-off: within that window, the query itself is the first exchange on the
connection. If the link died meanwhile, the query fails with
`DatabaseConnectionError` instead of being sent again on a new connection
(fbfast never repeats SQL that may have reached the server). After the window,
a dead connection is detected by the transaction start and replaced before any
SQL is sent.

## Measured on a controlled link

Disposable Firebird 5.0.3 server in Docker, toxiproxy adding the round-trip
time, macOS arm64, CPython 3.14, release builds; medians after warm-up.
`firebird-driver` 2.0.3 reuses one connection and prepared statements without
resetting the session. Raw data and full tables (0, 5, 20 and 50 ms):
`benchmarks/results/20261106T154216Z-latency-0.3.0-final/comparison.md` (all
cases, 3 samples) and `benchmarks/results/20261106T162154Z-warm-0.3.0/comparison.md`
(small queries, 5 samples after up to 8 warm-up calls).

| Case (20 ms RTT) | 0.2.1 | 0.3.0 | firebird-driver |
|---|---:|---:|---:|
| One row by primary key | 91 ms | 22 ms | 67 ms |
| 1,000 rows with a small UTF8 text BLOB | 28.3 s | 0.19 s | 0.22 s |
| same, ISO8859_1 text BLOB | 28.8 s | 0.20 s | 0.23 s |
| 100 rows with a 20 KiB text BLOB | 8.1 s | 0.45 s | 0.51 s |
| `LIST()` inside `WITH` (100 groups) | 2.6 s | 0.04 s | 0.09 s |
| 1,000 narrow rows | 134 ms | 49 ms | 111 ms |
| 32 small queries at once (8 connections) | 0.47 s | 0.19 s | 0.43 s |
| 128 small queries at once (8 connections) | 1.84 s | 0.82 s | 1.47 s |

Large results (100,000 rows, 64 columns) are bounded by the server's batch
size, not by fbfast; see `TcpRemoteBufferSize` below.

## BLOB columns

Firebird 5.0.3+ (wire protocol 19) sends BLOBs up to `max_inline_blob_size`
bytes (65,535 by default) together with their row, and the client keeps them
in a per-connection cache (`blob_cache_size`, 10 MiB by default). The client
uses that copy only when the BLOB is opened **without** a BLOB parameter
buffer. fbfast 0.2.1 opened every text BLOB with a transliteration buffer, so
each one cost an extra round trip. 0.3.0 opens UTF8, ISO8859_1, NONE/OCTETS
and binary BLOBs raw (ISO8859_1 is converted to UTF-8 locally; this was
checked against the server for all 256 byte values). Other text charsets
(WIN1252, ...) still use the server filter and cost one round trip per value.

Checklist for BLOB-heavy queries over a WAN:

- `pool.session_info()["inline_blobs"]` must be `True` (server 5.0.3 or later;
  prefer 5.0.4, because 5.0.3 keeps inline BLOB memory until the transaction
  ends, [#8673](https://github.com/FirebirdSQL/firebird/issues/8673)).
- BLOBs larger than 65,535 bytes are always fetched separately.
- `LIST()` returns a BLOB; it is now inline too.
- If a short text column is stored as BLOB, `CAST(col AS VARCHAR(n))` with the
  smallest `n` that fits. Avoid `VARCHAR(8191)` by habit: in UTF8 a row grows to
  about 32 KB and the server sends only ~32 rows per round trip.
- Streams with BLOB columns are batched by decoded bytes (`batch_bytes`).

## Several queries

- Independent queries: `pool.run_all([...])` / `run_all_async`. With N small
  queries and `pool_size` connections, the total is about
  `ceil(N / pool_size)` × one query. Unlike `asyncio.gather`, it never exceeds
  the pool's waiting queue (264 calls by default).
- Queries that must see the same data: `pool.snapshot()`. Each query inside
  costs about one round trip (no per-query transaction or reset).

## Wire settings

| Setting | Where it applies | Recommendation |
|---|---|---|
| `TcpRemoteBufferSize` | **server** `firebird.conf` | The server's value bounds how much data each fetch round trip carries (default 8192; 32767 is the maximum). Setting it only on the client changed nothing in our test: 100,000 rows at 20 ms took 101 round trips and 3.73 s either way. |
| `WireCompression` | client asks (`wire_compression=True`), server agrees | Helps on slow or metered links, costs CPU on a fast LAN. Check `session_info()["wire_compressed"]`. Works on Linux (system `libz.so.1`), Windows (bundled `zlib1.dll`) and macOS (0.3.0 ships a shim; the official macOS client looked for a non-existent `libz.dylib.1`). |
| `WireCrypt` | both | Keep `wire_crypt="required"`. Encryption costs CPU and about one round trip when connecting, nothing per query. Use `"disabled"` only on a reviewed transport (WireGuard/Tailscale already encrypt). |
| `TcpNoNagle` | both | Already the default. |
| `ConnectionTimeout` | client | Use `connect_timeout`. |
| `DummyPacketInterval` | server | The server probing silent clients; it does not reduce latency. fbfast's idle ping (`health_check_after`) keeps tunnels and NAT mappings alive. |
| `AuthClient` | client | The bundled runtime uses `Srp256, Srp`. Do not add `Legacy_Auth`. |
| `ClientBatchBuffer` | client | Only for the batch write API; irrelevant to a read-only client. |

With 1 MiB/s and 20 ms, compression took a 100-row 20 KiB-BLOB query from
2.6 s to 0.085 s and 100,000 narrow rows from 5.3 s to 3.2 s.

## On a fast LAN

Conversion to Python objects is a visible part of the time on localhost and
LANs. Prefer `fetch_json`, `fetch_arrow`/`fetch_polars` or streams when the
consumer does not need Python objects per row; tuples are cheaper than
dictionaries, and dictionaries cheaper than Pydantic models.
