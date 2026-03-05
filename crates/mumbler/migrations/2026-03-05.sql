CREATE TABLE `images` (
    `id` BLOB PRIMARY KEY,
    `width` INTEGER NOT NULL,
    `height` INTEGER NOT NULL,
    `content_type` TEXT NOT NULL,
    `data` BLOB NOT NULL
);

CREATE TABLE `config` (
    `key` TEXT PRIMARY KEY,
    `value` BLOB NOT NULL
);
