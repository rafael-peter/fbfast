-- Only run through run_comparison.py --prepare on the owned disposable server.
-- Deterministic formula (no random generator or seed-dependent implementation).
CREATE TABLE FBFAST_BENCH (
    ID INTEGER NOT NULL PRIMARY KEY,
    V INTEGER NOT NULL,
    TXT VARCHAR(48) CHARACTER SET UTF8
);
SET TERM ^;
EXECUTE BLOCK AS DECLARE I INTEGER = 1;
BEGIN
    WHILE (I <= 1000000) DO BEGIN
        INSERT INTO FBFAST_BENCH VALUES (
            :I, MOD(:I * 17, 100003) - 50001,
            CASE WHEN MOD(:I, 7) = 0 THEN NULL ELSE 'Olá 日本語 ' || :I END
        );
        I = I + 1;
    END
END^
SET TERM ;^
GRANT SELECT ON FBFAST_BENCH TO FBFAST_READONLY;
COMMIT;
