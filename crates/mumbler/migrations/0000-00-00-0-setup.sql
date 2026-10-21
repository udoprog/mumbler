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

CREATE TABLE `auth_users` (
    `id` INTEGER PRIMARY KEY,
    `login` TEXT UNIQUE,
    `email` TEXT UNIQUE,
    `role` TEXT NOT NULL,
    `access` TEXT NOT NULL,
    `method` TEXT NOT NULL,
    `password_hash` TEXT,
    `google_sub` TEXT UNIQUE,
    `created_at` INTEGER NOT NULL
);

CREATE TABLE `auth_sessions` (
    `id` TEXT PRIMARY KEY,
    `user_id` INTEGER NOT NULL,
    `impersonator_id` INTEGER,
    `created_at` INTEGER NOT NULL,
    `expires_at` INTEGER NOT NULL
);

CREATE INDEX `auth_sessions_user` ON `auth_sessions` (`user_id`);
CREATE INDEX `auth_sessions_impersonator` ON `auth_sessions` (`impersonator_id`);

CREATE TABLE `auth_links` (
    `id` TEXT PRIMARY KEY,
    `user_id` INTEGER NOT NULL,
    `kind` TEXT NOT NULL,
    `expires_at` INTEGER NOT NULL,
    `used_at` INTEGER
);

CREATE INDEX `auth_links_user` ON `auth_links` (`user_id`);

CREATE TABLE `auth_settings` (
    `key` TEXT PRIMARY KEY,
    `value` TEXT NOT NULL
);
