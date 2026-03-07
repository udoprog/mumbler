CREATE TABLE `images` (
    `id` BLOB PRIMARY KEY,
    `width` INTEGER NOT NULL,
    `height` INTEGER NOT NULL,
    `content_type` TEXT NOT NULL,
    `data` BLOB NOT NULL
);

CREATE TABLE `config` (
    `id` INTEGER NOT NULL,
    `key` INTEGER NOT NULL,
    `value` BLOB NOT NULL,
    PRIMARY KEY (`id`, `key`)
);
