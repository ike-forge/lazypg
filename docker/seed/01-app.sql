-- Ordinary application schema: keys, foreign keys, indexes, a view.
CREATE SCHEMA shop;

CREATE TABLE shop.customer (
    id         bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    email      text NOT NULL UNIQUE,
    name       text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE shop.product (
    sku   text PRIMARY KEY,
    title text NOT NULL,
    price numeric(10, 2) NOT NULL CHECK (price >= 0),
    tags  text[] NOT NULL DEFAULT '{}'
);

CREATE TABLE shop.order_line (
    order_id    bigint NOT NULL,
    line_no     int NOT NULL,
    customer_id bigint NOT NULL REFERENCES shop.customer (id),
    sku         text NOT NULL REFERENCES shop.product (sku),
    quantity    int NOT NULL CHECK (quantity > 0),
    PRIMARY KEY (order_id, line_no)
);
CREATE INDEX order_line_customer_idx ON shop.order_line (customer_id);

CREATE VIEW shop.customer_totals AS
SELECT c.id, c.email, sum(p.price * l.quantity) AS total
FROM shop.customer c
JOIN shop.order_line l ON l.customer_id = c.id
JOIN shop.product p ON p.sku = l.sku
GROUP BY c.id, c.email;

INSERT INTO shop.customer (email, name)
SELECT 'user' || g || '@example.com', 'User ' || g
FROM generate_series(1, 500) g;

INSERT INTO shop.product (sku, title, price, tags)
SELECT 'SKU-' || g, 'Product ' || g, (g % 97) + 0.99, ARRAY['tag' || (g % 5)]
FROM generate_series(1, 200) g;

INSERT INTO shop.order_line (order_id, line_no, customer_id, sku, quantity)
SELECT g / 3, g % 3, (g % 500) + 1, 'SKU-' || ((g % 200) + 1), (g % 7) + 1
FROM generate_series(3, 20000) g;
