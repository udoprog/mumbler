use core::ffi::c_int;
use core::str;

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use api::ImageId;
use jiff::Timestamp;
use musli::Encode;
use musli::alloc::Global;
use musli::de::DecodeOwned;
use musli::mode::Binary;
use relative_path::{RelativePath, RelativePathBuf};
use rust_embed::RustEmbed;
use sqll::{BIND_INDEX, Bind, BindValue, FromColumn, Statement, ty};
use sqll::{OpenOptions, SendStatement};
use tokio::sync::Mutex;
use tokio::task;

#[derive(Clone, Copy)]
struct IntegerBlob(u64);

impl BindValue for IntegerBlob {
    #[inline]
    fn bind_value(&self, stmt: &mut Statement, index: c_int) -> Result<(), sqll::Error> {
        self.0.bind_value(stmt, index)
    }
}

impl Bind for IntegerBlob {
    #[inline]
    fn bind(&self, stmt: &mut Statement) -> Result<(), sqll::Error> {
        self.bind_value(stmt, BIND_INDEX)
    }
}

impl FromColumn<'_> for IntegerBlob {
    type Type = ty::Blob;

    #[inline]
    fn from_column(stmt: &Statement, index: ty::Blob) -> Result<Self, sqll::Error> {
        let id = u64::from_le_bytes(<[u8; 8]>::from_column(stmt, index)?);
        Ok(IntegerBlob(id))
    }
}

use crate::Paths;

#[derive(RustEmbed)]
#[folder = "migrations"]
struct Migrations;

struct Inner {
    list_images: SendStatement,
    insert_image: SendStatement,
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
                insert_image: c.prepare("INSERT INTO images (id, data) VALUES (?, ?)")?.into_send()?,
                get_config: c.prepare("SELECT value FROM config WHERE key = ?")?.into_send()?,
                set_config: c.prepare("INSERT INTO config (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")?.into_send()?,
            }
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    /// Save an image to the database, returning its unique identifier.
    pub(crate) async fn save_image(&self, data: Vec<u8>) -> Result<ImageId> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            let id = IntegerBlob(rand::random());
            inner.insert_image.execute((id, data))?;
            Ok(ImageId::new(id.0))
        });

        task.await?
    }

    /// List images.
    async fn images(&self) -> Result<Vec<ImageId>> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            let mut images = Vec::new();

            while let Some(IntegerBlob(id)) = inner.list_images.next::<IntegerBlob>()? {
                images.push(ImageId::new(id));
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
