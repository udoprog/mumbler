CREATE TABLE `images` (
    `id` BLOB PRIMARY KEY,
    `data` BLOB NOT NULL
);

CREATE TABLE `config` (
    `key` TEXT PRIMARY KEY,
    `value` BLOB NOT NULL
);
