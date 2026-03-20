use core::str;

use std::collections::HashSet;
use std::fs;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use api::{
    Color, ContentType, Extent, Id, Image, ImageWithData, Key, Pan, RemoteId, Transform, Type,
    Value, ValueKind, ValueType, Vec3,
};
use jiff::Timestamp;
use musli::alloc::Global;
use musli::de::DecodeOwned;
use musli::mode::Binary;
use musli::{Encode, descriptive};
use relative_path::{RelativePath, RelativePathBuf};
use rust_embed::RustEmbed;
use sqll::{OpenOptions, SendStatement};
use tokio::sync::Mutex;
use tokio::task;

macro_rules! value_kind_switch {
    ($self:expr, $value:expr, ($($args:expr),*), $add:ident, $delete:ident) => {
        match $value.into_kind() {
            ValueKind::String(string) => {
                $self.$add($($args),*, string).await?;
            }
            ValueKind::Float(value) => {
                $self.$add($($args),*, value).await?;
            }
            ValueKind::Integer(value) => {
                $self.$add($($args),*, value).await?;
            }
            ValueKind::Boolean(value) => {
                $self.$add($($args),*, value).await?;
            }
            ValueKind::Bytes(value) => {
                $self.$add($($args),*, value).await?;
            }
            ValueKind::Id(value) => {
                if value.is_zero() {
                    $self.$delete($($args),*).await?;
                } else {
                    $self.$add($($args),*, value).await?;
                }
            }
            ValueKind::RemoteId(value) => {
                if value.id.is_zero() {
                    $self.$delete($($args),*).await?;
                } else {
                    $self.$add($($args),*, value).await?;
                }
            }
            ValueKind::Transform(value) => {
                $self.$add($($args),*, value).await?;
            }
            ValueKind::Color(value) => {
                $self.$add($($args),*, value).await?;
            }
            ValueKind::Vec3(value) => {
                $self.$add($($args),*, value).await?;
            }
            ValueKind::Extent(extent) => {
                $self.$add($($args),*, extent).await?;
            }
            ValueKind::Pan(pan) => {
                $self.$add($($args),*, pan).await?;
            }
            ValueKind::Empty => {
                $self.$delete($($args),*).await?;
            }
        }
    };
}

use crate::Paths;

#[derive(RustEmbed)]
#[folder = "migrations"]
struct Migrations;

struct Inner {
    scratch: Vec<u8>,
    insert_image: SendStatement,
    select_images: SendStatement,
    select_images_with_data: SendStatement,
    select_image_data: SendStatement,
    delete_image: SendStatement,
    get_property: SendStatement,
    list_properties: SendStatement,
    set_property: SendStatement,
    delete_property: SendStatement,
    get_config: SendStatement,
    set_config: SendStatement,
    delete_config: SendStatement,
    list_configs: SendStatement,
    insert_object: SendStatement,
    delete_object: SendStatement,
    list_objects: SendStatement,
    list_members: SendStatement,
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
                insert_image: c.prepare("INSERT INTO images (id, content_type, data, width, height) VALUES (?, ?, ?, ?, ?)")?.into_send()?,
                select_images: c.prepare("SELECT id, content_type, width, height FROM images")?.into_send()?,
                select_images_with_data: c.prepare("SELECT id, content_type, data, width, height FROM images")?.into_send()?,
                select_image_data: c.prepare("SELECT data FROM images WHERE id = ?")?.into_send()?,
                delete_image: c.prepare("DELETE FROM images WHERE id = ?")?.into_send()?,
                get_property: c.prepare("SELECT value FROM properties WHERE id = ? AND key = ?")?.into_send()?,
                list_properties: c.prepare("SELECT key, value FROM properties WHERE id = ?")?.into_send()?,
                set_property: c.prepare("INSERT INTO properties (id, key, value) VALUES (?, ?, ?) ON CONFLICT(id, key) DO UPDATE SET value = excluded.value")?.into_send()?,
                delete_property: c.prepare("DELETE FROM properties WHERE id = ? AND key = ?")?.into_send()?,
                get_config: c.prepare("SELECT value FROM config WHERE key = ?")?.into_send()?,
                set_config: c.prepare("INSERT INTO config (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")?.into_send()?,
                delete_config: c.prepare("DELETE FROM config WHERE key = ?")?.into_send()?,
                list_configs: c.prepare("SELECT key, value FROM config")?.into_send()?,
                insert_object: c.prepare("INSERT INTO objects (id, type) VALUES (?, ?)")?.into_send()?,
                delete_object: c.prepare("DELETE FROM objects WHERE id = ?")?.into_send()?,
                list_objects: c.prepare("SELECT id, type, group_id FROM objects")?.into_send()?,
                list_members: c.prepare("SELECT id FROM objects WHERE group_id = ?")?.into_send()?,
            }
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    /// Get an image by its unique identifier.
    pub(crate) async fn get_image_data(&self, id: Id) -> Result<Option<Vec<u8>>> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.select_image_data.bind((id,))?;

            if let Some(data) = inner.select_image_data.next::<Vec<u8>>()? {
                return Ok(Some(data));
            }

            Ok(None)
        });

        task.await?
    }

    /// List all images in the database.
    pub(crate) async fn images(&self) -> Result<Vec<Image>> {
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

    /// List all images in the database.
    pub(crate) async fn images_with_data(&self) -> Result<Vec<ImageWithData>> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.select_images_with_data.reset()?;

            let mut images = Vec::new();

            while let Some(image) = inner.select_images_with_data.next::<ImageWithData>()? {
                images.push(image);
            }

            Ok(images)
        });

        task.await?
    }

    /// Delete an image from the database by its unique identifier.
    pub(crate) async fn delete_image(&self, id: Id) -> Result<()> {
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
        data: Vec<u8>,
        width: u32,
        height: u32,
    ) -> Result<Id> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner
                .insert_image
                .execute((id, content_type, data, width, height))?;
            Ok(id)
        });

        task.await?
    }

    pub async fn config<T>(&self, key: Key) -> Result<Option<T>>
    where
        T: 'static + Send + DecodeOwned<Binary, Global>,
    {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.get_config.bind((key,))?;

            if let Some(row) = inner.get_config.next::<&[u8]>()? {
                let value = descriptive::from_slice::<T>(row)?;
                return Ok(Some(value));
            }

            Ok(None)
        });

        task.await?
    }

    pub async fn set_optional_config<T>(&self, key: Key, value: Option<T>) -> Result<()>
    where
        T: 'static + Send + Encode<Binary>,
    {
        if let Some(value) = value {
            self.set_config(key, value).await
        } else {
            self.delete_config(key).await
        }
    }

    /// Set the specified configuration.
    pub async fn set_config_value(&self, key: Key, value: Value) -> Result<()> {
        value_kind_switch!(self, value, (key), set_config, delete_config);
        Ok(())
    }

    pub async fn set_config<T>(&self, key: Key, value: T) -> Result<()>
    where
        T: 'static + Send + Encode<Binary>,
    {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            descriptive::encode(&mut inner.scratch, &value)?;

            let Inner {
                set_config,
                scratch,
                ..
            } = &mut *inner;

            set_config.execute((key, &scratch[..]))?;
            scratch.clear();
            Ok(())
        });

        task.await?
    }

    pub async fn delete_config(&self, key: Key) -> Result<()> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.delete_config.execute((key,))?;
            Ok(())
        });

        task.await?
    }

    /// Get specific property.
    ///
    /// Properties are values associated with a specified object identified by id.
    pub async fn property<T>(&self, id: Id, key: Key) -> Result<Option<T>>
    where
        T: 'static + Send + DecodeOwned<Binary, Global>,
    {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.get_property.bind((id, key))?;

            if let Some(row) = inner.get_property.next::<&[u8]>()? {
                let value = descriptive::from_slice::<T>(row)?;
                return Ok(Some(value));
            }

            Ok(None)
        });

        task.await?
    }

    /// Set specific configuration by key, or delete it if the value is unset.
    pub async fn set_property_value(&self, id: Id, key: Key, value: Value) -> Result<()> {
        value_kind_switch!(self, value, (id, key), set_property, delete_property);
        Ok(())
    }

    /// Set specific configuration by key, or delete it if the value is `None`.
    pub async fn set_optional_property(
        &self,
        id: Id,
        key: Key,
        value: Option<impl 'static + Send + Encode<Binary>>,
    ) -> Result<()> {
        if let Some(value) = value {
            self.set_property(id, key, value).await
        } else {
            self.delete_property(id, key).await
        }
    }

    /// Set specific configuration by key.
    pub async fn set_property(
        &self,
        id: Id,
        key: Key,
        value: impl 'static + Send + Encode<Binary>,
    ) -> Result<()> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            descriptive::encode(&mut inner.scratch, &value)?;

            let Inner {
                set_property,
                scratch,
                ..
            } = &mut *inner;

            set_property.execute((id, key, &scratch[..]))?;
            scratch.clear();
            Ok(())
        });

        task.await?
    }

    /// Remove the specified configuration.
    pub async fn delete_property(&self, id: Id, key: Key) -> Result<()> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.delete_property.execute((id, key))?;
            Ok(())
        });

        task.await?
    }

    /// Insert an object into the database.
    pub async fn insert_object(&self, id: Id, ty: Type) -> Result<()> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.insert_object.execute((id, ty))?;
            Ok(())
        });

        task.await?
    }

    /// Delete an object in the database.
    pub async fn delete_object(&self, id: Id) -> Result<()> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.delete_object.execute((id,))?;
            Ok(())
        });

        task.await?
    }

    /// List all objects in the database.
    pub async fn objects(&self) -> Result<Vec<(Id, Type, Option<Id>)>> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.list_objects.reset()?;

            let mut objects = Vec::new();

            while let Some((id, ty, group_id)) =
                inner.list_objects.next::<(Id, Type, Option<Id>)>()?
            {
                objects.push((id, ty, group_id));
            }

            Ok(objects)
        });

        task.await?
    }

    /// List the members of a group.
    pub async fn members(&self, group_id: Id) -> Result<Vec<Id>> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.list_members.bind((group_id,))?;

            let mut members = Vec::new();

            while let Some(member_id) = inner.list_members.next::<Id>()? {
                members.push(member_id);
            }

            Ok(members)
        });

        task.await?
    }

    pub async fn properties(&self, id: Id) -> Result<Vec<(Key, Value)>> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.list_properties.bind((id,))?;

            let mut props = Vec::new();

            while let Some((key, value)) = inner.list_properties.next::<(Key, &[u8])>()? {
                let Some(ty) = key.ty() else {
                    continue;
                };

                tracing::debug!(?id, ?key, "Loading property");
                let value = value_from_blob(ty, value)?;
                props.push((key, value));
            }

            Ok(props)
        });

        task.await?
    }

    pub async fn configs(&self) -> Result<Vec<(Key, Value)>> {
        let mut inner = self.inner.clone().lock_owned().await;

        let task = task::spawn_blocking(move || {
            inner.list_configs.reset()?;

            let mut props = Vec::new();

            while let Some((key, value)) = inner.list_configs.next::<(Key, &[u8])>()? {
                let Some(ty) = key.ty() else {
                    continue;
                };

                let value =
                    value_from_blob(ty, value).with_context(|| anyhow!("decoding {key}"))?;
                props.push((key, value));
            }

            Ok(props)
        });

        task.await?
    }
}

fn value_from_blob(ty: ValueType, blog: &[u8]) -> Result<Value> {
    let value = match ty {
        ValueType::Id => Value::from(descriptive::from_slice::<Id>(blog)?),
        ValueType::String => Value::from(descriptive::from_slice::<String>(blog)?),
        ValueType::Float => Value::from(descriptive::from_slice::<f64>(blog)?),
        ValueType::Integer => Value::from(descriptive::from_slice::<i64>(blog)?),
        ValueType::Pan => Value::from(descriptive::from_slice::<Pan>(blog)?),
        ValueType::Extent => Value::from(descriptive::from_slice::<Extent>(blog)?),
        ValueType::Transform => Value::from(descriptive::from_slice::<Transform>(blog)?),
        ValueType::Vec3 => Value::from(descriptive::from_slice::<Vec3>(blog)?),
        ValueType::Color => Value::from(descriptive::from_slice::<Color>(blog)?),
        ValueType::Bytes => Value::from(descriptive::from_slice::<Vec<u8>>(blog)?),
        ValueType::Boolean => Value::from(descriptive::from_slice::<bool>(blog)?),
        ValueType::RemoteId => Value::from(descriptive::from_slice::<RemoteId>(blog)?),
    };

    Ok(value)
}
