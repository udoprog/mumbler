CREATE TABLE `objects` (
    `id` INTEGER NOT NULL,
    `type` INTEGER NOT NULL,
    PRIMARY KEY (`id`, `type`)
);

CREATE TABLE `properties` (
    `id` INTEGER NOT NULL,
    `key` INTEGER NOT NULL,
    `value` NOT NULL,
    PRIMARY KEY (`id`, `key`),
    FOREIGN KEY (`id`) REFERENCES `objects`(`id`) ON DELETE CASCADE
);

CREATE TABLE `images` (
    `id` INTEGER PRIMARY KEY,
    `content_type` INTEGER NOT NULL,
    `role` INTEGER NOT NULL,
    `width` INTEGER NOT NULL,
    `height` INTEGER NOT NULL,
    `data` BLOB NOT NULL
);
