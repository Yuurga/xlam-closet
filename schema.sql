-- Схема базы данных «шкафа» (PostgreSQL).
-- Идентична схеме из постановки задачи, с поправкой под PostgreSQL:
-- BLOB -> BYTEA.

CREATE TABLE IF NOT EXISTS places (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT
);

CREATE TABLE IF NOT EXISTS items (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    place_id    BIGINT NOT NULL
                REFERENCES places(id) ON DELETE RESTRICT,
    name        TEXT NOT NULL,
    description TEXT,
    photo       BYTEA
);

CREATE TABLE IF NOT EXISTS attributes (
    id   BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS attribute_values (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    attribute_id BIGINT NOT NULL
                 REFERENCES attributes(id) ON DELETE CASCADE,
    value        TEXT NOT NULL,

    UNIQUE (attribute_id, value)
);

CREATE TABLE IF NOT EXISTS item_attribute_values (
    item_id            BIGINT NOT NULL
                       REFERENCES items(id) ON DELETE CASCADE,
    attribute_value_id BIGINT NOT NULL
                       REFERENCES attribute_values(id) ON DELETE CASCADE,

    PRIMARY KEY (item_id, attribute_value_id)
);

CREATE INDEX IF NOT EXISTS idx_items_place_id
    ON items(place_id);

CREATE INDEX IF NOT EXISTS idx_item_attribute_values_value_item
    ON item_attribute_values(attribute_value_id, item_id);
