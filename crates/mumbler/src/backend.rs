use core::fmt;
use core::num::NonZeroU32;
use core::sync::atomic::Ordering;

use core::sync::atomic::AtomicU32;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;
use api::{
    AtomicIds, ContentType, Id, Key, PeerId, Properties, PublicKey, RemoteId, RemoteObject,
    RemoteUpdateBody, Role, StableId, Transform, Type, UpdateBody, Value,
};
use musli_web::api::ChannelId;
use musli_web::ws::Channels;
use parking_lot::RwLock as BlockingRwLock;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{Mutex, MutexGuard, Notify, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::crypto;
use crate::crypto::Keypair;

use super::{Database, Paths};

#[derive(Debug, Clone)]
pub(crate) enum Broadcast {
    Update(api::UpdateBody),
    RemoteUpdate(api::RemoteUpdateBody),
    Notification(api::NotificationBody),
}

impl From<api::UpdateBody> for Broadcast {
    #[inline]
    fn from(value: api::UpdateBody) -> Self {
        Self::Update(value)
    }
}

impl From<api::RemoteUpdateBody> for Broadcast {
    #[inline]
    fn from(value: api::RemoteUpdateBody) -> Self {
        Self::RemoteUpdate(value)
    }
}

impl From<api::NotificationBody> for Broadcast {
    #[inline]
    fn from(value: api::NotificationBody) -> Self {
        Self::Notification(value)
    }
}

/// State for the backend.
#[derive(Debug, Clone)]
pub(crate) struct LocalObject {
    pub(crate) ty: Type,
    pub(crate) id: Id,
    pub(crate) props: Properties,
    pub(crate) changed: HashSet<Key>,
}

/// Local image data.
#[derive(Debug, Clone)]
pub(crate) struct LocalImage {
    /// The id of the image.
    pub id: Id,
    /// The content type of the image.
    pub content_type: ContentType,
    /// The role of the image.
    pub role: Role,
    /// The width of the image.
    pub width: u32,
    /// The height of the image.
    pub height: u32,
    /// The bytes of the image.
    pub bytes: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct PeerInfo {
    /// The public key of the peer.
    pub(crate) public_key: PublicKey,
    /// Objects associated with the peer.
    pub(crate) objects: HashMap<Id, RemoteObject>,
    /// Images associated with the peer.
    pub(crate) images: HashSet<Id>,
    /// Properties associated with the peer.
    pub(crate) props: Properties,
}

/// Information about remote peers.
pub(crate) struct ClientState {
    /// Set of used identifiers.
    pub(crate) used_ids: HashSet<NonZeroU32>,
    /// The keypair associated with this client.
    pub(crate) keypair: Keypair,
    /// Remote objects.
    pub(crate) peers: HashMap<PeerId, PeerInfo>,
    /// Local objects.
    pub(crate) objects: HashMap<Id, LocalObject>,
    /// Identifiers of objects that have been changed.
    pub(crate) objects_changed: HashSet<Id>,
    /// Identifiers of objects that have been added.
    pub(crate) objects_added: HashSet<Id>,
    /// Identifiers of objects that have been deleted.
    pub(crate) objects_removed: HashSet<Id>,
    /// Local images.
    pub(crate) images: HashMap<Id, LocalImage>,
    /// Identifiers of images that have been added.
    pub(crate) images_added: HashSet<Id>,
    /// Identifiers of images that have been deleted.
    pub(crate) images_removed: HashSet<Id>,
    /// Remote properties.
    pub(crate) props: Properties,
    /// Collection of properties that have changed.
    pub(crate) props_changed: HashSet<Key>,
}

impl ClientState {
    #[inline]
    pub(crate) fn to_stable_id(&self, peer_id: PeerId, id: Id) -> StableId {
        let Some(peer) = self.peers.get(&peer_id) else {
            return StableId::ZERO;
        };

        StableId::new(peer.public_key, id)
    }
}

struct Data {
    bytes: Box<[u8]>,
}

/// Temporary images.
#[derive(Default)]
pub(crate) struct ImageCache {
    images: HashMap<RemoteId, Data>,
}

impl ImageCache {
    /// Store an image.
    pub(crate) fn store(&mut self, id: RemoteId, bytes: Box<[u8]>) {
        self.images.insert(id, Data { bytes });
    }

    /// Remove an image.
    pub(crate) fn remove(&mut self, id: &RemoteId) {
        self.images.remove(id);
    }

    /// Get an image.
    pub(crate) fn get(&self, id: &RemoteId) -> Option<&[u8]> {
        Some(self.images.get(id)?.bytes.as_ref())
    }
}

/// State communicated to the mumblelink plugin.
pub(crate) struct MumblelinkState {
    pub(crate) transform: Option<Transform>,
}

struct Inner {
    channels: Channels,
    ids: AtomicIds,
    database: Database,
    #[allow(unused)]
    paths: Paths,
    client_state: Mutex<ClientState>,
    client_notify: Notify,
    client_restart_notify: Notify,
    mumblelink_state: Mutex<MumblelinkState>,
    mumblelink_notify: Notify,
    mumblelink_restart_notify: Notify,
    image_cache: RwLock<ImageCache>,
    broadcast: Sender<Broadcast>,
    mumble_object: AtomicId,
    hidden: BlockingRwLock<HashSet<Id>>,
}

/// The backend of the application, containing the database and other shared state.
#[derive(Clone)]
pub struct Backend {
    inner: Arc<Inner>,
}

impl Backend {
    /// Construct a new backend.
    pub async fn new(database: Database, paths: Paths) -> Result<Self> {
        let (broadcast, _) = tokio::sync::broadcast::channel(128);

        tracing::debug!("loading objects from database");

        let mut used_ids = HashSet::new();
        let mut objects = HashMap::new();
        let mut images = HashMap::new();
        let mut hidden = HashSet::new();

        for (id, ty) in database.objects().await? {
            let mut props = Properties::new();

            for (key, value) in database.properties(id).await? {
                match key {
                    Key::HIDDEN => {
                        if value.as_bool() {
                            hidden.insert(id);
                        }
                    }
                    _ => {}
                }

                props.insert(key, value);
            }

            let mut object = LocalObject {
                ty,
                id,
                props,
                changed: HashSet::new(),
            };

            // Migrate existing database objects to ensure they have an
            // established sort.
            if !object.props.contains(Key::SORT) {
                let sort = object.id.to_vec();
                object.props.insert(Key::SORT, Value::from(sort));
            }

            objects.insert(id, object);

            if let Some(id) = id.to_non_zero_u32() {
                used_ids.insert(id);
            }
        }

        let mut props = Properties::new();

        for (key, value) in database.properties(Id::ZERO).await? {
            props.insert(key, value);
        }

        let client_secret = match props.get(Key::PEER_SECRET).as_str() {
            "" => {
                let secret = crypto::random_string();

                database
                    .set_property(Id::ZERO, Key::PEER_SECRET, Value::from(secret.clone()))
                    .await?;

                props.insert(Key::PEER_SECRET, Value::from(secret.clone()));
                secret
            }
            secret => secret.to_owned(),
        };

        let keypair = crypto::derive_keypair(client_secret.as_bytes());

        let mut image_cache = ImageCache::default();

        for image in database.images_with_data().await? {
            tracing::debug! {
                ?image.id,
                ?image.content_type,
                bytes = image.bytes.len(),
                image.width,
                image.height,
                "loading image",
            };

            image_cache.store(RemoteId::local(image.id), Box::from(image.bytes.as_slice()));

            images.insert(
                image.id,
                LocalImage {
                    id: image.id,
                    content_type: image.content_type,
                    bytes: image.bytes,
                    width: image.width,
                    height: image.height,
                    role: image.role,
                },
            );

            if let Some(id) = image.id.to_non_zero_u32() {
                used_ids.insert(id);
            }
        }

        let mumble_object = props.get(Key::MUMBLE_OBJECT).as_id();

        tracing::debug!("loaded {} objects", objects.len());

        Ok(Self {
            inner: Arc::new(Inner {
                channels: Channels::default(),
                ids: AtomicIds::new(rand::random()),
                database,
                paths,
                client_state: Mutex::new(ClientState {
                    used_ids,
                    keypair,
                    peers: HashMap::new(),
                    objects,
                    objects_changed: HashSet::new(),
                    objects_added: HashSet::new(),
                    objects_removed: HashSet::new(),
                    images,
                    images_added: HashSet::new(),
                    images_removed: HashSet::new(),
                    props,
                    props_changed: HashSet::new(),
                }),
                image_cache: RwLock::new(image_cache),
                client_notify: Notify::new(),
                client_restart_notify: Notify::new(),
                mumblelink_state: Mutex::new(MumblelinkState { transform: None }),
                mumblelink_notify: Notify::new(),
                mumblelink_restart_notify: Notify::new(),
                broadcast,
                mumble_object: AtomicId::new(mumble_object),
                hidden: BlockingRwLock::new(hidden),
            }),
        })
    }

    #[inline]
    pub(crate) fn channels(&self) -> &Channels {
        &self.inner.channels
    }

    /// Generate a new unique identifier.
    async fn new_id(&self) -> Result<Id> {
        loop {
            let Some(id) = self.inner.ids.next() else {
                bail!("exhausted all possible identifiers");
            };

            if self.inner.client_state.lock().await.used_ids.insert(id) {
                return Ok(Id::new(id.get()));
            }
        }
    }

    /// Get a reference to the database.
    pub(crate) fn db(&self) -> &Database {
        &self.inner.database
    }

    /// Set up an event subscriber.
    pub(crate) fn subscribe(&self) -> Receiver<Broadcast> {
        self.inner.broadcast.subscribe()
    }

    /// Load the id of the object used to position mumble.
    pub(crate) fn mumble_object(&self) -> Id {
        self.inner.mumble_object.load()
    }

    /// Test if the given object is hidden.
    pub(crate) fn is_hidden(&self, id: Id) -> bool {
        self.inner.hidden.read().contains(&id)
    }

    /// Set whether the given object is hidden.
    pub(crate) fn set_hidden(&self, id: Id, hidden: bool) {
        if hidden {
            self.inner.hidden.write().insert(id);
        } else {
            self.inner.hidden.write().remove(&id);
        }
    }

    /// Set the id of the object used to position mumble.
    pub(crate) fn store_mumble_object(&self, id: Id) {
        self.inner.mumble_object.store(id);
    }

    /// Lock the remote state and access it.
    pub(crate) async fn client_state(&self) -> MutexGuard<'_, ClientState> {
        self.inner.client_state.lock().await
    }

    /// Write temporary images.
    pub(crate) async fn write_images(&self) -> RwLockWriteGuard<'_, ImageCache> {
        self.inner.image_cache.write().await
    }

    /// Read temporary images.
    pub(crate) async fn read_images(&self) -> RwLockReadGuard<'_, ImageCache> {
        self.inner.image_cache.read().await
    }

    /// Broadcast an event to all peers.
    pub(crate) fn broadcast(&self, ev: impl Into<Broadcast>) {
        let result = self.inner.broadcast.send(ev.into());

        if let Err(error) = result {
            tracing::error!(%error, "failed to broadcast event");
        }
    }

    /// Broadcast an info notification to all connected web clients.
    pub(crate) fn notify_info(&self, component: impl fmt::Display, message: impl fmt::Display) {
        self.broadcast(api::NotificationBody::Info {
            component: component.to_string(),
            message: message.to_string(),
        });
    }

    /// Broadcast an error notification to all connected web clients.
    pub(crate) fn notify_error(&self, component: impl fmt::Display, message: impl fmt::Display) {
        self.broadcast(api::NotificationBody::Error {
            component: component.to_string(),
            message: message.to_string(),
        });
    }

    /// Receive the next event.
    pub(crate) async fn client_wait(&self) {
        self.inner.client_notify.notified().await;
    }

    /// Receive the next transform update.
    pub(crate) async fn mumblelink_wait(&self) {
        self.inner.mumblelink_notify.notified().await;
    }

    /// Update position and front.
    pub(crate) async fn object_update(&self, id: Id, values: &[(Key, Value)]) {
        let mut state = self.inner.client_state.lock().await;

        let Some(object) = state.objects.get_mut(&id) else {
            return;
        };

        for (key, value) in values {
            object.props.insert(*key, value.clone());
            object.changed.insert(*key);
        }

        if !values.is_empty() {
            state.objects_changed.insert(id);
            self.inner.client_notify.notify_one();
        }
    }

    /// Get the transform.
    pub(crate) async fn mumblelink_state(&self) -> MutexGuard<'_, MumblelinkState> {
        self.inner.mumblelink_state.lock().await
    }

    /// Set the transform for mumblelink.
    pub(crate) async fn set_mumblelink_transform(&self, transform: Option<Transform>) {
        let mut state = self.inner.mumblelink_state.lock().await;
        state.transform = transform;
        self.inner.mumblelink_notify.notify_one();
    }

    /// Restart the mumblelink connection.
    pub(crate) fn restart_mumblelink(&self) {
        self.inner.mumblelink_restart_notify.notify_one();
    }

    /// Wait for a mumblelink restart signal.
    pub(crate) async fn mumblelink_restart_wait(&self) {
        self.inner.mumblelink_restart_notify.notified().await;
    }

    /// Signal the remote client to restart (re-read server config from DB).
    pub(crate) fn restart_client(&self) {
        self.inner.client_restart_notify.notify_one();
    }

    /// Wait for a client restart signal.
    pub(crate) async fn client_restart_wait(&self) {
        self.inner.client_restart_notify.notified().await;
    }

    pub(crate) async fn updates(&self, channel: ChannelId, values: &[(Key, Value)]) -> Result<()> {
        let mut restart_mumblelink = false;
        let mut restart_client = false;
        let mut public_key = None;

        let mut state = self.client_state().await;

        for (key, value) in values {
            match *key {
                Key::MUMBLE_ENABLED => {
                    restart_mumblelink = true;
                }
                Key::REMOTE_ENABLED | Key::REMOTE_SERVER | Key::REMOTE_TLS => {
                    restart_client = true;
                }
                Key::MUMBLE_OBJECT => {
                    let mumble_object = value.as_id();
                    self.store_mumble_object(mumble_object);

                    let transform = 'transform: {
                        if mumble_object.is_zero() {
                            break 'transform None;
                        };

                        let Some(object) = state.objects.get(&mumble_object) else {
                            break 'transform None;
                        };

                        if object.props.get(Key::HIDDEN).as_bool() {
                            None
                        } else {
                            object.props.get(Key::TRANSFORM).as_transform()
                        }
                    };

                    self.set_mumblelink_transform(transform).await;
                }
                Key::PEER_SECRET => {
                    let peer_secret = value.as_str();

                    if peer_secret.is_empty() {
                        continue;
                    }

                    let keypair = crypto::derive_keypair(peer_secret.as_bytes());
                    state.keypair = keypair;
                    restart_client = true;
                    public_key = Some(state.keypair.public_key());
                }
                _ => {}
            }

            state.props.insert(*key, value.clone());

            if key.is_remote() {
                state.props_changed.insert(*key);
                self.inner.client_notify.notify_one();
            }
        }

        drop(state);

        for (key, value) in values {
            self.broadcast(UpdateBody::Config {
                channel,
                key: *key,
                value: value.clone(),
            });
        }

        if let Some(public_key) = public_key {
            self.broadcast(UpdateBody::PublicKey { public_key });
        }

        if restart_mumblelink {
            self.restart_mumblelink();
        }

        if restart_client {
            self.restart_client();
        }

        Ok(())
    }

    /// Create a new local object, persisting it to the database and inserting
    /// it into the in-memory client state. Returns the new object's ID.
    pub(crate) async fn create_object(&self, ty: Type, props: Properties) -> Result<RemoteObject> {
        let id = self.new_id().await?;

        self.db().insert_object(id, ty).await?;

        let mut object = LocalObject {
            ty,
            id,
            props,
            changed: HashSet::new(),
        };

        let props = {
            let mut state = self.inner.client_state.lock().await;

            if !object.ty.is_global() {
                let last = state
                    .objects
                    .values()
                    .map(|o| o.props.get(Key::SORT).as_bytes())
                    .max();

                let sort = match last {
                    Some(sort) => sorting::after(sort),
                    None => object.id.to_vec(),
                };

                let sort = Value::from(sort);
                object.props.insert(Key::SORT, sort.clone());
            }

            let props = object.props.clone();
            state.objects.insert(id, object);
            state.objects_added.insert(id);
            props
        };

        for (key, value) in props.iter() {
            self.db().set_property(id, key, value.clone()).await?;
        }

        self.inner.client_notify.notify_one();
        Ok(RemoteObject { ty, id, props })
    }

    /// Remove a local object, removing it from the database and in-memory state.
    pub(crate) async fn remove_object(&self, id: Id) -> Result<()> {
        // If the mumble object is removed, clear the mumble object setting to
        // avoid dangling references.
        if self.mumble_object() == id {
            self.store_mumble_object(Id::ZERO);
            self.set_mumblelink_transform(None).await;
        }

        self.db().remove_object(id).await?;

        let mut state = self.inner.client_state.lock().await;

        if *state.props.get(Key::ROOM).as_stable_id()
            == StableId::new(state.keypair.public_key(), id)
        {
            self.db().remove_property(Id::ZERO, Key::ROOM).await?;

            state.props.remove(Key::ROOM);
            state.props_changed.insert(Key::ROOM);
        }

        state.objects.remove(&id);
        state.objects_changed.remove(&id);
        state.objects_added.remove(&id);
        state.objects_removed.insert(id);
        self.inner.client_notify.notify_one();
        Ok(())
    }

    pub(crate) async fn insert_image(
        &self,
        content_type: ContentType,
        role: Role,
        width: u32,
        height: u32,
        bytes: Vec<u8>,
    ) -> Result<Id> {
        let id = self.new_id().await?;

        let image = LocalImage {
            id,
            content_type,
            bytes,
            width,
            height,
            role,
        };

        self.db()
            .save_image(
                image.id,
                image.content_type,
                image.role,
                image.width,
                image.height,
                image.bytes.clone(),
            )
            .await?;

        {
            let mut state = self.inner.client_state.lock().await;
            state.images.insert(id, image.clone());
            state.images_added.insert(id);
            self.inner.client_notify.notify_one();
        }

        {
            let mut images = self.inner.image_cache.write().await;
            images.store(RemoteId::local(id), Box::from(image.bytes.as_slice()));
        }

        Ok(id)
    }

    /// Remove a local image.
    pub(crate) async fn remove_image(&self, id: Id) -> Result<()> {
        let mut state = self.inner.client_state.lock().await;
        let state = &mut *state;

        self.db().remove_image(id).await?;

        state.images.remove(&id);
        state.images_added.remove(&id);
        state.images_removed.insert(id);
        self.inner.client_notify.notify_one();

        let mut any = false;

        // For any object that has the image set as a property, remove the property.
        for object in state.objects.values_mut() {
            for key in [Key::IMAGE_ID, Key::ROOM_BACKGROUND] {
                let image_id = object.props.get(key).as_id();

                if image_id != id {
                    continue;
                }

                any = true;

                object.props.remove(key);
                object.changed.insert(key);
                state.objects_changed.insert(object.id);

                self.db().remove_property(object.id, key).await?;

                self.broadcast(RemoteUpdateBody::ObjectUpdated {
                    channel: ChannelId::NONE,
                    id: RemoteId::local(object.id),
                    key,
                    value: Value::empty(),
                });
            }
        }

        if any {
            self.inner.client_notify.notify_one();
        }

        Ok(())
    }
}

/// An optional and atomically stored identifier.
pub struct AtomicId {
    raw: AtomicU32,
}

impl AtomicId {
    fn new(value: Id) -> Self {
        Self {
            raw: AtomicU32::new(value.get()),
        }
    }

    /// Load the identifier from the atomic, returning None if it is invalid.
    pub fn load(&self) -> Id {
        Id::new(self.raw.load(Ordering::Relaxed))
    }

    /// Store an identifier in the atomic, replacing any existing value.
    pub fn store(&self, id: Id) {
        self.raw.store(id.get(), Ordering::Relaxed);
    }
}
