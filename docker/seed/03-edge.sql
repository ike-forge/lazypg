-- Identifier and rendering edge cases.
CREATE SCHEMA "Edge Cases";

CREATE TABLE "Edge Cases"."Mixed Case" ("Id" int PRIMARY KEY, "select" text, "ünïcødé" text);
INSERT INTO "Edge Cases"."Mixed Case" VALUES
(1, 'reserved word column', 'e' || chr(769) || ' decomposed'),
(2, repeat('wide ', 200), '日本語の文字列'),
(3, '', '🙂 emoji');

CREATE TABLE "Edge Cases".no_pk (a int, b text);
INSERT INTO "Edge Cases".no_pk SELECT g, 'row ' || g FROM generate_series(1, 50) g;

CREATE MATERIALIZED VIEW "Edge Cases".no_pk_summary AS
SELECT count(*) AS n FROM "Edge Cases".no_pk;

CREATE TABLE "Edge Cases".measurement (
    city_id int NOT NULL,
    logdate date NOT NULL,
    peak    int
) PARTITION BY RANGE (logdate);
CREATE TABLE "Edge Cases".measurement_2026 PARTITION OF "Edge Cases".measurement
    FOR VALUES FROM ('2026-01-01') TO ('2027-01-01');

-- Cancellation: SELECT pg_sleep(30);
