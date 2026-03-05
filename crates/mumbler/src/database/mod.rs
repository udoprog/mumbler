use core::str;

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use api::ImageId;
use jiff::Timestamp;
use musli::alloc::Global;
use musli::de::DecodeOwned;
use musli::mode::Binary;
use musli::{Decode, Encode};
use relative_path::{RelativePath, RelativePathBuf};
use rust_embed::RustEmbed;
use sqll::{OpenOptions, SendStatement};
use tokio::sync::Mutex;
use tokio::task;

use crate::Paths;

#[derive(RustEmbed)]
#[folder = "migrations"]
struct Migrations;

struct Inner {
    list_images: SendStatement,
    get_config: SendStatement,
    set_config: SendStatement,
}

/// A database connection.
pub struct Database {
    inner: Arc<Mutex<Inner>>,
}

impl Database {
    /// Open a database at the given paths prepared for migrations.
    pub fn open(paths: &Paths, memory: bool) -> Result<Self> {
        let mut open = OpenOptions::new();
        open.create().read_write().no_mutex();

        let c = if memory {
            open.open_in_memory()?
        } else {
            open.open(&paths.db)?
        };

        let count = c.prepare("SELECT COUNT(*) FROM `sqlite_master` WHERE `type` = 'table' AND `name` = 'migrations'")?.next::<i64>()?.unwrap_or(0);

        if count == 0 {
            c.execute(
                "CREATE TABLE `migrations` (
                    `id` TEXT PRIMARY KEY,
                    `applied_at` INTEGER NOT NULL
                )",
            )?;
        }

        let mut applied = HashSet::new();

        for row in c.prepare("SELECT id FROM migrations")?.iter::<String>() {
            applied.insert(RelativePathBuf::from(row?));
        }

        let mut to_run = Vec::new();

        for id in Migrations::iter() {
            let path = RelativePath::new(id.as_ref());

            if !matches!(path.extension(), Some("sql")) {
                continue;
            }

            if !applied.contains(path) {
                to_run.push(path.to_owned());
            }
        }

        to_run.sort();

        for path in to_run {
            tracing::info!(?path, "Applying migration");

            let sql = Migrations::get(path.as_str())
                .expect("embedded migration")
                .data;
            let sql = str::from_utf8(&sql)?;

            c.execute("BEGIN TRANSACTION")?;
            c.execute(sql)
                .with_context(|| anyhow!("migration {path}"))?;
            c.execute("COMMIT TRANSACTION")?;

            let mut insert = c.prepare("INSERT INTO migrations (id, applied_at) VALUES (?, ?)")?;

            let now = Timestamp::now();
            insert.execute((path.as_str(), now.as_millisecond()))?;
        }

        let inner = unsafe {
            Inner {
                list_images: c.prepare("SELECT id FROM images")?.into_send()?,
                get_config: c.prepare("SELECT value FROM config WHERE key = ?")?.into_send()?,
                set_config: c.prepare("INSERT INTO config (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")?.into_send()?,
            }
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    /// List images.
    async fn images(&self) -> Result<Vec<ImageId>> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            let mut images = Vec::new();

            while let Some(id) = inner.list_images.next::<ImageId>()? {
                images.push(id);
            }

            Ok(images)
        });

        task.await?
    }

    /// Get specific configuration by key.
    async fn get_config<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: 'static + Send + DecodeOwned<Binary, Global>,
    {
        let mut inner = self.inner.clone().lock_owned().await;

        let key = Box::<str>::from(key);

        let task = task::spawn_blocking(move || {
            inner.get_config.bind((key,))?;

            if let Some(row) = inner.get_config.next::<&[u8]>()? {
                let value = musli::storage::from_slice::<T>(&row)?;
                return Ok(Some(value));
            }

            Ok(None)
        });

        task.await?
    }

    /// Set specific configuration by key.
    async fn set_config<T>(&self, key: &str, value: T) -> Result<()>
    where
        T: 'static + Send + Encode<Binary>,
    {
        let mut inner = self.inner.clone().lock_owned().await;

        let key = Box::<str>::from(key);

        let task = task::spawn_blocking(move || {
            let value = musli::storage::to_vec(&value)?;
            inner.set_config.execute((key, value))?;
            Ok(())
        });

        task.await?
    }
}
