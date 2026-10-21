use std::fs;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use api::{
    Canvas2, Color, ContentType, Extent, Id, Key, PeerId, Role, StableId, Transform, Type, Value,
    ValueKind, ValueType, Vec3,
};
use musli::descriptive;
use relative_path::RelativePath;
use rust_embed::RustEmbed;
use sqll::{Connection, Index, OpenOptions, SendStatement};
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
    conn: Connection,
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

/// Bookkeeping for applied migrations, laid out as `sqll::Migrations` created
/// it so databases migrated by it keep their records.
const MIGRATIONS_INIT: &str =
    r#"CREATE TABLE IF NOT EXISTS "migrations" (id TEXT PRIMARY KEY, applied_at INTEGER NOT NULL)"#;

/// Whether this database has never held a schema.
///
/// Asked before the bookkeeping table is created, so a database that has been
/// migrated has the tables those migrations built, and one that has not has
/// nothing at all.
fn is_empty(c: &Connection) -> Result<bool> {
    let mut q = c.prepare(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' LIMIT 1",
    )?;

    Ok(q.first::<i64>(())?.is_none())
}

/// Embedded migration ids in the order they apply: filenames are date
/// prefixed, which puts the base schema, prefixed with zeroes, first.
fn migration_ids() -> Vec<String> {
    let mut ids = Migrations::iter()
        .filter(|id| matches!(RelativePath::new(id.as_ref()).extension(), Some("sql")))
        .map(|id| id.into_owned())
        .collect::<Vec<_>>();

    ids.sort();
    ids
}

/// Bring the schema up to date, returning how many migrations ran. Must be
/// called inside of an immediate transaction, so a migration and its record
/// land together and processes starting at once serialize on the write lock.
///
/// The base schema is *complete*: it spells out the whole current schema and is
/// edited in place as it changes. An empty database is built by it alone and
/// every later migration is recorded without being run.
///
/// A database that already holds a schema runs every embedded migration not
/// yet recorded, in filename order, and records each. The base is only ever
/// recorded there, never run, since such a database was built by an earlier
/// edition of it.
fn migrate(c: &Connection) -> Result<usize> {
    let empty = is_empty(c).context("checking whether the database is empty")?;

    c.execute(MIGRATIONS_INIT)
        .context("creating migrations table")?;

    let ids = migration_ids();

    let Some((base, rest)) = ids.split_first() else {
        return Ok(0);
    };

    if empty {
        apply(c, base)?;

        for id in rest {
            record(c, id)?;
        }

        return Ok(1);
    }

    let mut select = c.prepare("SELECT 1 FROM migrations WHERE id = ?")?;
    let mut applied = 0;

    for (n, id) in ids.iter().enumerate() {
        if select.first::<i64>(id.as_str())?.is_some() {
            continue;
        }

        if n == 0 {
            record(c, id)?;
        } else {
            apply(c, id)?;
            applied += 1;
        }
    }

    Ok(applied)
}

/// Run one migration and record it.
fn apply(c: &Connection, id: &str) -> Result<()> {
    let asset = Migrations::get(id).with_context(|| anyhow!("migration {id} not found"))?;
    let sql = str::from_utf8(&asset.data).with_context(|| anyhow!("migration {id}"))?;
    c.execute(sql).with_context(|| anyhow!("migration {id}"))?;
    record(c, id)
}

fn record(c: &Connection, id: &str) -> Result<()> {
    let mut insert = c.prepare(
        "INSERT INTO migrations (id, applied_at) VALUES (?, CAST(strftime('%s', 'now') AS INTEGER))",
    )?;

    insert
        .execute(id)
        .with_context(|| anyhow!("recording migration {id}"))?;

    Ok(())
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
        open.create().read_write();

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

        let applied = c.transaction(migrate)?;

        if applied > 0 {
            tracing::info!(applied, "applied migrations");
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
                conn: c,
            }
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    /// Runs `f` on the connection on a blocking thread, holding the lock that
    /// every other use of it takes.
    pub(crate) async fn with_connection<T>(
        &self,
        f: impl FnOnce(&Connection) -> T + Send + 'static,
    ) -> Result<T, task::JoinError>
    where
        T: Send + 'static,
    {
        let inner = self.inner.clone().lock_owned().await;
        task::spawn_blocking(move || f(&inner.conn)).await
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

            select.reset()?;
            select.bind((id,))?;

            let mut props = Vec::new();

            while select.step()?.is_row() {
                let key = select.column::<Key>(0)?;

                let Some(ty) = key.ty() else {
                    continue;
                };

                tracing::debug!(?id, ?key, "loading property");

                let value = match value_from_blob(ty, select, 1) {
                    Ok(value) => value,
                    Err(error) => {
                        tracing::error!(?id, ?key, error = ?error, "failed to load property value");
                        continue;
                    }
                };

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
    fn bind_value(&self, stmt: &mut sqll::Statement, index: Index) -> sqll::Result<()> {
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

fn value_from_blob(ty: ValueType, stmt: &mut SendStatement, column: usize) -> Result<Value> {
    let value = match (ty, stmt.column_type(column)) {
        (ValueType::Boolean, sqll::ValueType::INTEGER) => {
            Value::from(stmt.column::<i64>(column)? != 0)
        }
        (ValueType::Bytes, sqll::ValueType::BLOB) => Value::from(stmt.column::<Vec<u8>>(column)?),
        (ValueType::Color, sqll::ValueType::BLOB) => {
            Value::from(descriptive::from_slice::<Color>(stmt.column(column)?)?)
        }
        (ValueType::Extent, sqll::ValueType::BLOB) => {
            Value::from(descriptive::from_slice::<Extent>(stmt.column(column)?)?)
        }
        (ValueType::Float, sqll::ValueType::FLOAT) => Value::from(stmt.column::<f64>(column)?),
        (ValueType::Id, sqll::ValueType::INTEGER) => Value::from(stmt.column::<Id>(column)?),
        (ValueType::Integer, sqll::ValueType::INTEGER) => Value::from(stmt.column::<i64>(column)?),
        (ValueType::Canvas2, sqll::ValueType::BLOB) => {
            Value::from(descriptive::from_slice::<Canvas2>(stmt.column(column)?)?)
        }
        (ValueType::PeerId, sqll::ValueType::INTEGER) => {
            Value::from(stmt.column::<PeerId>(column)?)
        }
        (ValueType::StableId, sqll::ValueType::BLOB) => {
            Value::from(descriptive::from_slice::<StableId>(stmt.column(column)?)?)
        }
        (ValueType::String, sqll::ValueType::TEXT) => {
            Value::from(stmt.unsized_column::<str>(column)?)
        }
        (ValueType::Transform, sqll::ValueType::BLOB) => {
            Value::from(descriptive::from_slice::<Transform>(stmt.column(column)?)?)
        }
        (ValueType::Vec3, sqll::ValueType::BLOB) => {
            Value::from(descriptive::from_slice::<Vec3>(stmt.column(column)?)?)
        }
        (ty, found) => {
            return Err(anyhow!(
                "Database column {found} does not match type {ty:?}"
            ));
        }
    };

    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[tokio::test]
    async fn properties_can_be_loaded_repeatedly() -> Result<()> {
        let db = Database::open(&Paths::new(Path::new("unused")), true)?;

        let a = Id::new(1);
        let b = Id::new(2);

        db.set_property(a, Key::HIDDEN, Value::from(true)).await?;
        db.set_property(a, Key::NAME, Value::from("x")).await?;
        db.set_property(b, Key::HIDDEN, Value::from(false)).await?;

        for _ in 0..2 {
            let props = db.properties(a).await?;
            assert_eq!(props.len(), 2);
            assert!(props.iter().any(|(k, v)| *k == Key::HIDDEN && v.as_bool()));
            assert!(
                props
                    .iter()
                    .any(|(k, v)| *k == Key::NAME && v.as_str() == "x")
            );
        }

        let props = db.properties(b).await?;
        assert_eq!(props.len(), 1);
        assert!(!props[0].1.as_bool());
        Ok(())
    }

    /// The base schema as deployed databases were built from it, before
    /// 2026-10-21-0-auth.sql was folded into it.
    const OLDEST_SETUP: &str = r#"
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
"#;

    #[derive(Debug, PartialEq, sqll::Row)]
    struct Entry {
        ty: String,
        name: String,
        tbl_name: String,
        sql: Option<String>,
    }

    /// Everything a schema is made of, in a stable order.
    fn schema(c: &Connection) -> Result<Vec<Entry>> {
        let mut q = c.prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )?;

        let mut rows = Vec::new();

        while let Some(row) = q.next()? {
            rows.push(row);
        }

        Ok(rows)
    }

    fn recorded(c: &Connection) -> Result<Vec<String>> {
        let mut q = c.prepare("SELECT id FROM migrations ORDER BY id")?;
        let mut ids = Vec::new();

        while let Some(id) = q.next()? {
            ids.push(id);
        }

        Ok(ids)
    }

    #[test]
    fn fresh_and_upgraded_schemas_match() -> Result<()> {
        let fresh = Connection::open_in_memory()?;
        assert_eq!(fresh.transaction(migrate)?, 1);

        let upgraded = Connection::open_in_memory()?;
        sqll::Migrations::new().run(&upgraded, [("0000-00-00-0-setup.sql", OLDEST_SETUP)])?;
        assert_eq!(upgraded.transaction(migrate)?, migration_ids().len() - 1);

        assert_eq!(schema(&fresh)?, schema(&upgraded)?);
        assert_eq!(recorded(&fresh)?, migration_ids());
        assert_eq!(recorded(&upgraded)?, migration_ids());
        Ok(())
    }

    #[test]
    fn migrating_again_does_nothing() -> Result<()> {
        let c = Connection::open_in_memory()?;
        c.transaction(migrate)?;
        let before = schema(&c)?;

        assert_eq!(c.transaction(migrate)?, 0);
        assert_eq!(schema(&c)?, before);
        assert_eq!(recorded(&c)?, migration_ids());
        Ok(())
    }
}
