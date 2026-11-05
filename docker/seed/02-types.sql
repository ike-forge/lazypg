-- One column per type family the grid and inspector must render.
CREATE SCHEMA zoo;

CREATE TYPE zoo.mood AS ENUM ('sad', 'ok', 'happy');
CREATE TYPE zoo.point2 AS (x double precision, y double precision);

CREATE TABLE zoo.everything (
    id          int PRIMARY KEY,
    b           boolean,
    i2          smallint,
    i8          bigint,
    n           numeric,
    f8          double precision,
    t           text,
    vc          varchar(16),
    by          bytea,
    u           uuid,
    d           date,
    ts          timestamp,
    tstz        timestamptz,
    iv          interval,
    j           json,
    jb          jsonb,
    arr         int[],
    md          int[][],
    m           zoo.mood,
    p           zoo.point2,
    ip          inet,
    r           int4range,
    tsv         tsvector
);

INSERT INTO zoo.everything VALUES
(1, true, 1, 9223372036854775807, 3.14159265358979323846264338327950288, 'NaN',
 'plain', 'short', '\xdeadbeef', gen_random_uuid(), '2026-01-01', '2026-01-01 12:00',
 '2026-01-01 12:00+02', '1 year 2 mons 3 days', '{"a": 1}', '{"b": [1, 2]}',
 '{1,2,3}', '{{1,2},{3,4}}', 'happy', '(1.5,2.5)', '10.0.0.1/8', '[1,10)',
 to_tsvector('english', 'the quick brown fox')),
(2, NULL, NULL, NULL, NULL, 'Infinity', E'multi\nline\ttab', NULL, '', NULL,
 'infinity', NULL, NULL, NULL, 'null', 'null', '{}', NULL, NULL, NULL, NULL, 'empty', NULL);
