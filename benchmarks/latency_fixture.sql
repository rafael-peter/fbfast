-- Only run through run_latency.py --prepare on the owned disposable server.
-- Deterministic BLOB payloads for the latency benchmark (no random generator).
CREATE TABLE FBFAST_LAT_BLOB (
    ID INTEGER NOT NULL PRIMARY KEY,
    SMALL_TXT BLOB SUB_TYPE TEXT CHARACTER SET UTF8,
    MEDIUM_TXT BLOB SUB_TYPE TEXT CHARACTER SET UTF8,
    LARGE_TXT BLOB SUB_TYPE TEXT CHARACTER SET UTF8,
    BIN BLOB SUB_TYPE BINARY,
    LATIN_TXT BLOB SUB_TYPE TEXT CHARACTER SET ISO8859_1
);
SET TERM ^;
EXECUTE BLOCK AS
    DECLARE I INTEGER = 1;
    DECLARE J INTEGER;
    DECLARE CHUNK VARCHAR(1024) CHARACTER SET UTF8;
    DECLARE MEDIUM BLOB SUB_TYPE TEXT CHARACTER SET UTF8;
    DECLARE LARGE BLOB SUB_TYPE TEXT CHARACTER SET UTF8;
BEGIN
    WHILE (I <= 1000) DO BEGIN
        -- 1 KiB of ASCII text per chunk; medium = 20 KiB, large = 96 KiB (> 64 KiB inline limit).
        CHUNK = RPAD('row ' || :I || ' ', 1024, 'abcdefghijklmnopqrstuvwxyz0123456789');
        MEDIUM = NULL;
        LARGE = NULL;
        IF (:I <= 100) THEN BEGIN
            MEDIUM = '';
            J = 1;
            WHILE (J <= 20) DO BEGIN MEDIUM = MEDIUM || CHUNK; J = J + 1; END
        END
        IF (:I <= 20) THEN BEGIN
            LARGE = '';
            J = 1;
            WHILE (J <= 96) DO BEGIN LARGE = LARGE || CHUNK; J = J + 1; END
        END
        INSERT INTO FBFAST_LAT_BLOB VALUES (
            :I,
            'Olá 日本語 ' || :I || ' ' || RPAD('', 180, 'xyz'),
            :MEDIUM,
            :LARGE,
            CAST(RPAD('', 512, 'bin' || :I) AS VARCHAR(512) CHARACTER SET OCTETS),
            CAST('Ação São Paulo ' || :I || ' ' || RPAD('', 180, 'çãé') AS VARCHAR(400) CHARACTER SET ISO8859_1)
        );
        I = I + 1;
    END
END^
SET TERM ;^
GRANT SELECT ON FBFAST_LAT_BLOB TO FBFAST_READONLY;
COMMIT;
