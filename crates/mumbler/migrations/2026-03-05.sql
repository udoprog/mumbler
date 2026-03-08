CREATE TABLE `images` (
    `id` BLOB PRIMARY KEY,
    `width` INTEGER NOT NULL,
    `height` INTEGER NOT NULL,
    `content_type` TEXT NOT NULL,
    `data` BLOB NOT NULL
);

-- Global configuration.
CREATE TABLE `config` (
    `key` INTEGER NOT NULL PRIMARY KEY,
    `value` BLOB NOT NULL
);

-- Token-specific configuration.
CREATE TABLE `properties` (
    `id` INTEGER NOT NULL,
    `key` INTEGER NOT NULL,
    `value` BLOB NOT NULL,
    PRIMARY KEY (`id`, `key`),
    FOREIGN KEY (`id`) REFERENCES `tokens`(`id`) ON DELETE CASCADE
);

-- List of local objects.
CREATE TABLE `objects` (
    `id` INTEGER NOT NULL,
    `type` INTEGER NOT NULL,
    PRIMARY KEY (`id`, `type`)
);
