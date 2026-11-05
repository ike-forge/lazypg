-- A second database, for the tree's server level and switching to it.
CREATE DATABASE analytics;

\connect analytics

CREATE SCHEMA metrics;
CREATE TABLE metrics.event (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name text NOT NULL,
    at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO metrics.event (name) SELECT 'event ' || g FROM generate_series(1, 25) g;
