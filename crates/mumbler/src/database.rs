use core::ffi::c_int;
use core::str;

use std::collections::HashSet;
use std::fs;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use api::{
    Canvas2, Color, ContentType, Extent, Id, Key, PeerId, Role, StableId, Transform, Type, Value,
    ValueKind, ValueType, Vec3,
};
use jiff::Timestamp;
use musli::descriptive;
use relative_path::{RelativePath, RelativePathBuf};
use rust_embed::RustEmbed;
use sqll::{BIND_INDEX, OpenOptions, SendStatement};
use tokio::sync::Mutex;
use tokio::task;

#[derive(sqll::Row)]
pub(crate) struct Image {
    pub(crate) id: Id,
    pub(crate) content_type: ContentType,
    pub(crate) role: Role,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) bytes: Vec<u8>,
}

use crate::Paths;

#[derive(RustEmbed)]
#[folder = "migrations"]
struct Migrations;

struct Inner {
    scratch: Vec<u8>,
    delete_image: SendStatement,
    delete_object: SendStatement,
    delete_property: SendStatement,
    insert_image: SendStatement,
    insert_object: SendStatement,
    insert_property: SendStatement,
    select_images: SendStatement,
    select_objects: SendStatement,
    select_properties: SendStatement,
}

/// A database connection.
#[derive(Clone)]
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
            if let Some(parent) = paths.db.parent()
                && !parent.exists()
            {
                fs::create_dir_all(parent)?;
            }

            tracing::info!(path = ?paths.db.display(), "opening database");
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
            tracing::info!(?path, "applying migration");

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
                scratch: Vec::new(),
                delete_image: c.prepare("DELETE FROM images WHERE id = ?")?.into_send()?,
                delete_object: c.prepare("DELETE FROM objects WHERE id = ?")?.into_send()?,
                delete_property: c.prepare("DELETE FROM properties WHERE id = ? AND key = ?")?.into_send()?,
                insert_image: c.prepare("INSERT INTO images (id, content_type, data, width, height, role) VALUES (?, ?, ?, ?, ?, ?)")?.into_send()?,
                insert_object: c.prepare("INSERT INTO objects (id, type) VALUES (?, ?)")?.into_send()?,
                insert_property: c.prepare("INSERT INTO properties (id, key, value) VALUES (?, ?, ?) ON CONFLICT(id, key) DO UPDATE SET value = excluded.value")?.into_send()?,
                select_images: c.prepare("SELECT id, content_type, role, width, height, data FROM images")?.into_send()?,
                select_objects: c.prepare("SELECT id, type FROM objects")?.into_send()?,
                select_properties: c.prepare("SELECT key, value FROM properties WHERE id = ?")?.into_send()?,
            }
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    /// List all images in the database.
    pub(crate) async fn images_with_data(&self) -> Result<Vec<Image>> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.select_images.reset()?;

            let mut images = Vec::new();

            while let Some(image) = inner.select_images.next::<Image>()? {
                images.push(image);
            }

            Ok(images)
        });

        task.await?
    }

    /// Remove an image from the database by its unique identifier.
    pub(crate) async fn remove_image(&self, id: Id) -> Result<()> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.delete_image.execute((id,))?;
            Ok(())
        });

        task.await?
    }

    /// Save an image to the database, returning its unique identifier.
    pub(crate) async fn save_image(
        &self,
        id: Id,
        content_type: ContentType,
        role: Role,
        width: u32,
        height: u32,
        data: Vec<u8>,
    ) -> Result<Id> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner
                .insert_image
                .execute((id, content_type, data, width, height, role))?;
            Ok(id)
        });

        task.await?
    }

    /// Set specific configuration by key, or delete it if the value is unset.
    pub(crate) async fn set_property(&self, id: Id, key: Key, value: Value) -> Result<()> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            let Inner {
                insert_property: insert,
                delete_property: delete,
                scratch,
                ..
            } = &mut *inner;

            tracing::debug!(?id, ?key, ?value, "storing property");

            insert.reset()?;
            insert.bind_value(BIND_INDEX, id)?;
            insert.bind_value(BIND_INDEX + 1, key)?;

            match to_outcome(key, &value, &mut *scratch)? {
                Outcome::Remove => {
                    delete.execute((id, key))?;
                }
                Outcome::Insert(value) => {
                    insert.execute((id, key, value))?;
                    scratch.clear();
                }
            }

            Ok(())
        });

        task.await?
    }

    /// Set specific configuration by key, or delete it if the value is unset.
    pub(crate) async fn set_properties(
        &self,
        values: impl IntoIterator<Item = (Id, Key, Value)> + Send + 'static,
    ) -> Result<()> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            let Inner {
                insert_property: insert,
                delete_property: delete,
                scratch,
                ..
            } = &mut *inner;

            for (id, key, value) in values {
                tracing::debug!(?id, ?key, ?value, "storing property");

                match to_outcome(key, &value, &mut *scratch)? {
                    Outcome::Remove => {
                        delete.execute((id, key))?;
                    }
                    Outcome::Insert(value) => {
                        insert.execute((id, key, value))?;
                        scratch.clear();
                    }
                }
            }

            Ok(())
        });

        task.await?
    }

    /// Remove the specified configuration.
    pub(crate) async fn remove_property(&self, id: Id, key: Key) -> Result<()> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.delete_property.execute((id, key))?;
            Ok(())
        });

        task.await?
    }

    /// Insert an object into the database.
    pub(crate) async fn insert_object(&self, id: Id, ty: Type) -> Result<()> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.insert_object.execute((id, ty))?;
            Ok(())
        });

        task.await?
    }

    /// Remove an object in the database.
    pub(crate) async fn remove_object(&self, id: Id) -> Result<()> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.delete_object.execute((id,))?;
            Ok(())
        });

        task.await?
    }

    /// List all objects in the database.
    pub(crate) async fn objects(&self) -> Result<Vec<(Id, Type)>> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.select_objects.reset()?;

            let mut objects = Vec::new();

            while let Some((id, ty)) = inner.select_objects.next::<(Id, Type)>()? {
                tracing::debug!(?id, ?ty, "loading object");
                objects.push((id, ty));
            }

            Ok(objects)
        });

        task.await?
    }

    pub(crate) async fn properties(&self, id: Id) -> Result<Vec<(Key, Value)>> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            let Inner {
                select_properties: select,
                ..
            } = &mut *inner;

            select.bind((id,))?;

            let mut props = Vec::new();

            while select.step()?.is_row() {
                let key = select.column::<Key>(0)?;

                let Some(ty) = key.ty() else {
                    continue;
                };

                tracing::debug!(?id, ?key, "loading property");

                let value =
                    value_from_blob(ty, select).with_context(|| anyhow!("Decoding {key}"))?;

                props.push((key, value));
            }

            Ok(props)
        });

        task.await?
    }
}

enum Outcome<'a> {
    Remove,
    Insert(Insert<'a>),
}

enum Insert<'a> {
    String(&'a str),
    Float(f64),
    Integer(i64),
    Boolean(bool),
    Bytes(&'a [u8]),
    Id(Id),
    PeerId(PeerId),
    Encoded(&'a [u8]),
}

impl sqll::BindValue for Insert<'_> {
    #[inline]
    fn bind_value(&self, stmt: &mut sqll::Statement, index: c_int) -> sqll::Result<()> {
        match *self {
            Insert::String(value) => stmt.bind_value(index, value),
            Insert::Float(value) => stmt.bind_value(index, value),
            Insert::Integer(value) => stmt.bind_value(index, value),
            Insert::Boolean(value) => stmt.bind_value(index, value),
            Insert::Bytes(value) => stmt.bind_value(index, value),
            Insert::Id(value) => stmt.bind_value(index, value),
            Insert::PeerId(value) => stmt.bind_value(index, value),
            Insert::Encoded(value) => stmt.bind_value(index, value),
        }
    }
}

fn to_outcome<'a>(key: Key, value: &'a Value, scratch: &'a mut Vec<u8>) -> Result<Outcome<'a>> {
    let Some(ty) = key.ty() else {
        return Ok(Outcome::Remove);
    };

    match (ty, value.as_kind()) {
        (ValueType::String, ValueKind::String(value)) => {
            if value.is_empty() {
                Ok(Outcome::Remove)
            } else {
                Ok(Outcome::Insert(Insert::String(value)))
            }
        }
        (ValueType::Float, ValueKind::Float(value)) => Ok(Outcome::Insert(Insert::Float(*value))),
        (ValueType::Integer, ValueKind::Integer(value)) => {
            Ok(Outcome::Insert(Insert::Integer(*value)))
        }
        (ValueType::Boolean, ValueKind::Boolean(value)) => {
            Ok(Outcome::Insert(Insert::Boolean(*value)))
        }
        (ValueType::Bytes, ValueKind::Bytes(value)) => {
            if value.is_empty() {
                Ok(Outcome::Remove)
            } else {
                Ok(Outcome::Insert(Insert::Bytes(value)))
            }
        }
        (ValueType::Id, ValueKind::Id(value)) => {
            if value.is_zero() {
                Ok(Outcome::Remove)
            } else {
                Ok(Outcome::Insert(Insert::Id(*value)))
            }
        }
        (ValueType::StableId, ValueKind::StableId(value)) => {
            if value.id.is_zero() {
                Ok(Outcome::Remove)
            } else {
                descriptive::encode(&mut *scratch, &value)?;
                Ok(Outcome::Insert(Insert::Encoded(&scratch[..])))
            }
        }
        (ValueType::Transform, ValueKind::Transform(value)) => {
            descriptive::encode(&mut *scratch, &value)?;
            Ok(Outcome::Insert(Insert::Encoded(&scratch[..])))
        }
        (ValueType::Color, ValueKind::Color(value)) => {
            descriptive::encode(&mut *scratch, &value)?;
            Ok(Outcome::Insert(Insert::Encoded(&scratch[..])))
        }
        (ValueType::Vec3, ValueKind::Vec3(value)) => {
            descriptive::encode(&mut *scratch, &value)?;
            Ok(Outcome::Insert(Insert::Encoded(&scratch[..])))
        }
        (ValueType::Extent, ValueKind::Extent(value)) => {
            descriptive::encode(&mut *scratch, &value)?;
            Ok(Outcome::Insert(Insert::Encoded(&scratch[..])))
        }
        (ValueType::Canvas2, ValueKind::Canvas2(value)) => {
            descriptive::encode(&mut *scratch, &value)?;
            Ok(Outcome::Insert(Insert::Encoded(&scratch[..])))
        }
        (ValueType::PeerId, ValueKind::PeerId(value)) => {
            if value.is_zero() {
                Ok(Outcome::Remove)
            } else {
                Ok(Outcome::Insert(Insert::PeerId(*value)))
            }
        }
        (_, ValueKind::Empty) => Ok(Outcome::Remove),
        (ty, kind) => Err(anyhow!(
            "value kind {kind:?} does not match expected key type {ty:?}"
        )),
    }
}

fn value_from_blob(ty: ValueType, stmt: &mut SendStatement) -> Result<Value> {
    const COLUMN: c_int = 1;

    let value = match (ty, stmt.column_type(COLUMN)) {
        (ValueType::Boolean, sqll::ValueType::INTEGER) => {
            Value::from(stmt.column::<i64>(COLUMN)? != 0)
        }
        (ValueType::Bytes, sqll::ValueType::BLOB) => Value::from(stmt.column::<Vec<u8>>(COLUMN)?),
        (ValueType::Color, sqll::ValueType::BLOB) => {
            Value::from(descriptive::from_slice::<Color>(stmt.column(COLUMN)?)?)
        }
        (ValueType::Extent, sqll::ValueType::BLOB) => {
            Value::from(descriptive::from_slice::<Extent>(stmt.column(COLUMN)?)?)
        }
        (ValueType::Float, sqll::ValueType::FLOAT) => Value::from(stmt.column::<f64>(COLUMN)?),
        (ValueType::Id, sqll::ValueType::INTEGER) => Value::from(stmt.column::<Id>(COLUMN)?),
        (ValueType::Integer, sqll::ValueType::INTEGER) => Value::from(stmt.column::<i64>(COLUMN)?),
        (ValueType::Canvas2, sqll::ValueType::BLOB) => {
            Value::from(descriptive::from_slice::<Canvas2>(stmt.column(COLUMN)?)?)
        }
        (ValueType::PeerId, sqll::ValueType::INTEGER) => {
            Value::from(stmt.column::<PeerId>(COLUMN)?)
        }
        (ValueType::StableId, sqll::ValueType::BLOB) => {
            Value::from(descriptive::from_slice::<StableId>(stmt.column(COLUMN)?)?)
        }
        (ValueType::String, sqll::ValueType::TEXT) => {
            Value::from(stmt.unsized_column::<str>(COLUMN)?)
        }
        (ValueType::Transform, sqll::ValueType::BLOB) => {
            Value::from(descriptive::from_slice::<Transform>(stmt.column(COLUMN)?)?)
        }
        (ValueType::Vec3, sqll::ValueType::BLOB) => {
            Value::from(descriptive::from_slice::<Vec3>(stmt.column(COLUMN)?)?)
        }
        (ty, found) => {
            return Err(anyhow!(
                "Database column {found} does not match type {ty:?}"
            ));
        }
    };

    Ok(value)
}
