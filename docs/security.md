# Security model

fbfast is a read-oriented database client. Its controls reduce accidental
writes and bound common resource usage. They do not make untrusted SQL safe to
execute.

## SQL and database privileges

Every public query passes three gates: lexical validation, inspection of the
prepared statement type, and a Firebird `READ ONLY` transaction. This rejects
common DML, DDL, locking selects, multiple statements, and direct generator
calls. The gates are intentionally defense in depth because a statement type
alone is not a proof that a query has no effects.

A valid `SELECT` can invoke a selectable procedure, stored function, UDR, view,
or other server-side object. Those objects can contain privileged behavior.
Generator increments are non-transactional and survive rollback. Global
temporary tables and external routines introduce other boundaries. For this
reason, fbfast must not be used as a sandbox for arbitrary SQL.

Use a dedicated account and role with only the tables and views required by the
application. Avoid `EXECUTE` on procedures/functions and `USAGE` on generators
unless a reviewed query needs them. Do not grant DDL or administrative roles.
For SQL originating from users, public endpoints, or language models, select a
query from an application-owned allowlist and bind values as parameters instead
of accepting an arbitrary query string.

The disposable integration fixture proves both sides of this boundary: the
normal role cannot execute a privileged selectable procedure, while SYSDBA can
execute the same syntactically valid query and persist a generator increment
despite the read-only transaction rollback.

## Result and concurrency limits

`max_result_bytes` defaults to 512 MiB per materialized result. It covers native
row buffers that feed tuple, dictionary, and Pydantic results, plus JSON output
and Arrow or Polars batches. `max_result_rows` can add a row-count bound, and
`max_blob_bytes` limits an individual BLOB. Exceeding a limit fails the whole
operation and cleans up the cursor and transaction.

The byte count measures retained buffers in the Rust core. It is not an exact
RSS limit: allocators, temporary conversion buffers, Python objects, Pydantic,
and concurrent queries add overhead. A pool can materialize several results at
once, each with its own allowance. Set `max_result_bytes=None` only when another
layer enforces an appropriate bound.

Streams intentionally have no cumulative result-byte limit. Bound them with
`batch_bytes`, `prefetch_batches`, `max_blob_bytes`, consumer backpressure, and
query deadlines.

## Credentials and diagnostics

The core shares the configured password through a zeroizing secret buffer. The
temporary native C string is also overwritten immediately after the attach call.
Configuration debug output and serialization redact the password. Accessing
`config.password` creates a normal Python `str`; Python, the operating system,
and the Firebird client can make copies that fbfast cannot guarantee to erase.
Never log that property or include it in application diagnostics.

## Firebird versions and transport

Published wheels retain the pinned Firebird 5.0.3 client and test Firebird 5.0.3
and 5.0.4 servers. A client-only update to 5.0.4 was rejected during local
validation because named time-zone identifiers returned by a 5.0.3 server were
not representable by that client/runtime combination. A newer client would not
patch the server in any case. Where a server upgrade is not yet possible,
restrict network access, use a minimal role, remove unused routine and generator
privileges, and follow Firebird security releases.

The client decision was reviewed again for 0.3.0 (2026-11-06) against the nine
advisories published with Firebird 5.0.4 on 2026-04-17 (CVE-2026-35215,
CVE-2026-34232, CVE-2026-33337, CVE-2026-40342, CVE-2026-28224, CVE-2026-28214,
CVE-2026-28212, GHSA-6crx-4g37-7j49 and CVE-2025-65104). All of them describe a
client, usually unauthenticated, crashing or abusing a **server**; the last one
affects only Firebird 3 clients. The XDR routines involved are shared with the
client, so a malicious server could in theory reach the slice parsers, but
fbfast never requests ARRAY slices and treats the server as trusted. None of
them is a reason to ship the 5.0.4 client, so 0.3.0 keeps 5.0.3. Upgrade the
**server** to 5.0.4 or later to close these issues.

Wire encryption defaults to `required`. `enabled` permits negotiation to an
unencrypted connection, while `disabled` explicitly turns encryption off.
Keep `required` unless the deployment has a reviewed transport boundary.

## Reporting

Do not include credentials, production database contents, or operational
connection strings in a report. Provide a minimal reproducer, affected version,
platform, and expected boundary through the repository's private security
reporting channel when available.
