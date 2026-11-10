"""Connect with FB_* environment variables and run a few basic queries.

Set FB_HOST, FB_DATABASE, FB_USER, FB_PASSWORD (and optionally FB_PORT, FB_ROLE)
or put them in a .env file, then run:

    python examples/basic_query.py

Every query below reads system tables, so it works against any Firebird 5 database.
"""
from __future__ import annotations

import fbfast

TABLES = """
SELECT RDB$RELATION_ID, TRIM(RDB$RELATION_NAME)
FROM RDB$RELATIONS
WHERE COALESCE(RDB$SYSTEM_FLAG, 0) = ?
ORDER BY RDB$RELATION_ID
ROWS 5
"""


def main() -> None:
    # Explicit arguments win over FB_* variables, which win over .env.
    config = fbfast.FirebirdConfig(diagnostics=True)
    with fbfast.Pool(config) as pool:  # opens min_idle connections; closes them on exit
        info = pool.session_info()
        assert info is not None  # set once a connection exists
        print(f"Firebird {info['server_version']} over {info['network_protocol']}, "
              f"encrypted={info['wire_encrypted']}, inline BLOBs={info['inline_blobs']}")

        # First column of the first row (or None).
        count = pool.fetch_val("SELECT COUNT(*) FROM RDB$RELATIONS")
        print(f"{count} relations")

        # One row as a dict; parameters are positional ``?`` placeholders.
        who = pool.fetch_one(
            "SELECT CURRENT_USER AS USER_NAME, CURRENT_ROLE AS ROLE_NAME FROM RDB$DATABASE",
            as_dict=True,
        )
        print(f"connected as {who}")

        # Many rows as tuples. CHAR columns keep their padding unless trimmed.
        for relation_id, name in pool.fetch_all(TABLES, [1]):
            print(f"  system table {relation_id:>3}  {name}")

        # The plan, without executing the query.
        print(pool.explain("SELECT RDB$RELATION_NAME FROM RDB$RELATIONS WHERE RDB$RELATION_ID = ?",
                           [1]).strip())

        # Errors carry phase, sqlstate and native Firebird codes.
        try:
            pool.fetch_val("SELECT NO_SUCH_COLUMN FROM RDB$DATABASE")
        except fbfast.QueryError as error:
            print(f"expected error: {type(error).__name__} phase={error.phase} "
                  f"sqlstate={error.sqlstate}")

        # Counters and per-query traces (diagnostics=True) cost no round trip.
        trace = pool.diagnostics()[-1]
        print(f"last query: ok={trace['ok']} roundtrips={trace['roundtrips']} "
              f"phases={[phase['phase'] for phase in trace['phases']]}")
        stats = pool.stats()
        print(f"pool: {stats['acquires']} acquires, {stats['wire_roundtrips']} round trips")


if __name__ == "__main__":
    main()
