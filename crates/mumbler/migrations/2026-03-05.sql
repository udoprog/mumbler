CREATE TABLE `images` (
    `id` BLOB PRIMARY KEY,
    `image` BLOB
);

CREATE TABLE `config` (
    `key` TEXT PRIMARY KEY,
    `value` BLOB NOT NULL
);
