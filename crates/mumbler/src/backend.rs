use core::fmt;
use core::sync::atomic::Ordering;

use core::sync::atomic::AtomicU64;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use api::ContentType;
use api::Properties;
use api::{Id, Key, PeerId, RemoteObject, Transform, Type, Value};
use parking_lot::RwLock as BlockingRwLock;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{Mutex, MutexGuard, Notify, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::{Database, Paths};

#[derive(Debug, Clone)]
pub(crate) struct LocalConfigEvent {
    pub(crate) body: api::UpdateBody,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalUpdateEvent {
    pub(crate) body: api::LocalUpdateBody,
}

#[derive(Debug, Clone)]
pub(crate) enum BackendEvent {
    ConfigUpdate(LocalConfigEvent),
    LocalUpdate(LocalUpdateEvent),
    RemoteUpdate(api::RemoteUpdateBody),
    Notification {
        error: bool,
        component: String,
        message: String,
    },
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
    /// The bytes of the image.
    pub bytes: Vec<u8>,
    /// The width of the image.
    pub width: u32,
    /// The height of the image.
    pub height: u32,
}

#[derive(Default)]
pub(crate) struct PeerInfo {
    /// Objects associated with the peer.
    pub(crate) objects: HashMap<Id, RemoteObject>,
    /// Images associated with the peer.
    pub(crate) images: HashSet<Id>,
    /// Properties associated with the peer.
    pub(crate) props: Properties,
}

/// Information about remote peers.
pub(crate) struct ClientState {
    /// Remote objects.
    pub(crate) peers: HashMap<PeerId, PeerInfo>,
    /// Local objects.
    pub(crate) objects: HashMap<Id, LocalObject>,
    /// Identifiers of objects that have been changed.
    pub(crate) objects_changed: HashSet<Id>,
    /// Identifiers of objects that have been added.
    pub(crate) objects_added: HashSet<Id>,
    /// Identifiers of objects that have been deleted.
    pub(crate) objects_deleted: HashSet<Id>,
    /// Local images.
    pub(crate) images: HashMap<Id, LocalImage>,
    /// Identifiers of images that have been added.
    pub(crate) images_added: HashSet<Id>,
    /// Identifiers of images that have been deleted.
    pub(crate) images_deleted: HashSet<Id>,
    /// Remote properties.
    pub(crate) props: Properties,
    /// Collection of properties that have changed.
    pub(crate) props_changed: HashSet<Key>,
}

struct Data {
    peer_id: PeerId,
    bytes: Box<[u8]>,
}

/// Temporary images.
pub(crate) struct Images {
    images: HashMap<Id, Data>,
}

impl Images {
    /// Store an image and return its ID.
    pub(crate) fn store(&mut self, peer_id: PeerId, id: Id, bytes: Box<[u8]>) -> Id {
        self.images.insert(id, Data { peer_id, bytes });
        id
    }

    /// Remove an image by ID.
    pub(crate) fn remove(&mut self, peer_id: PeerId, id: Id) {
        if let Some(data) = self.images.get(&id)
            && data.peer_id == peer_id
        {
            self.images.remove(&id);
        }
    }

    /// Get an image by ID.
    pub(crate) fn get(&self, id: &Id) -> Option<&[u8]> {
        Some(self.images.get(id)?.bytes.as_ref())
    }
}

/// State communicated to the mumblelink plugin.
pub(crate) struct MumblelinkState {
    pub(crate) transform: Option<Transform>,
}

struct Inner {
    database: Database,
    #[allow(unused)]
    paths: Paths,
    client_state: Mutex<ClientState>,
    client_notify: Notify,
    client_restart_notify: Notify,
    mumblelink_state: Mutex<MumblelinkState>,
    mumblelink_notify: Notify,
    mumblelink_restart_notify: Notify,
    images: RwLock<Images>,
    broadcast: Sender<BackendEvent>,
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
    pub async fn new(db: Database, paths: Paths) -> Result<Self> {
        let (broadcast, _) = tokio::sync::broadcast::channel(16);

        tracing::debug!("Loading objects from database");

        let mut objects = HashMap::new();
        let mut images = HashMap::new();
        let mut hidden = HashSet::new();

        for (id, ty, group_id) in db.objects().await? {
            tracing::debug!(?id, ?ty, "Loading object");

            let mut props = Properties::new();

            props.insert(Key::GROUP, Value::from(group_id));

            for (key, value) in db.properties(id).await? {
                tracing::debug!(?id, ?key, ?value, "Loading property");

                match key {
                    Key::HIDDEN => {
                        if value.as_bool().unwrap_or_default() {
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
        }

        for image in db.images_with_data().await? {
            tracing::debug! {
                ?image.id,
                ?image.content_type,
                bytes = image.bytes.len(),
                image.width,
                image.height,
                "loading image",
            };

            images.insert(
                image.id,
                LocalImage {
                    id: image.id,
                    content_type: image.content_type,
                    bytes: image.bytes,
                    width: image.width,
                    height: image.height,
                },
            );
        }

        let mumble_object = db.config(Key::MUMBLE_OBJECT).await?.unwrap_or_default();

        tracing::debug!("Loaded {} objects", objects.len());

        Ok(Self {
            inner: Arc::new(Inner {
                database: db,
                paths,
                client_state: Mutex::new(ClientState {
                    peers: HashMap::new(),
                    objects,
                    objects_changed: HashSet::new(),
                    objects_added: HashSet::new(),
                    objects_deleted: HashSet::new(),
                    images,
                    images_added: HashSet::new(),
                    images_deleted: HashSet::new(),
                    props: Properties::new(),
                    props_changed: HashSet::new(),
                }),
                images: RwLock::new(Images {
                    images: HashMap::new(),
                }),
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

    /// Get a reference to the database.
    pub(crate) fn db(&self) -> &Database {
        &self.inner.database
    }

    /// Set up an event subscriber.
    pub(crate) fn subscribe(&self) -> Receiver<BackendEvent> {
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
    pub(crate) async fn images(&self) -> RwLockWriteGuard<'_, Images> {
        self.inner.images.write().await
    }

    /// Read temporary images.
    pub(crate) async fn images_read(&self) -> RwLockReadGuard<'_, Images> {
        self.inner.images.read().await
    }

    /// Broadcast an event to all peers.
    pub(crate) fn broadcast(&self, ev: BackendEvent) {
        let _ = self.inner.broadcast.send(ev);
    }

    /// Broadcast an info notification to all connected web clients.
    pub(crate) fn notify_info(&self, component: impl fmt::Display, message: impl fmt::Display) {
        self.broadcast(BackendEvent::Notification {
            error: false,
            component: component.to_string(),
            message: message.to_string(),
        });
    }

    /// Broadcast an error notification to all connected web clients.
    pub(crate) fn notify_error(&self, component: impl fmt::Display, message: impl fmt::Display) {
        self.broadcast(BackendEvent::Notification {
            error: true,
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

    /// Update a property, this will filter properties that should not be
    /// propagated to other peers.
    pub(crate) async fn update(&self, key: Key, value: Value) -> Result<()> {
        self.db().set_config_value(key, value.clone()).await?;

        if !matches!(key, Key::PEER_NAME) {
            return Ok(());
        }

        let mut state = self.inner.client_state.lock().await;
        state.props.insert(key, value);
        state.props_changed.insert(key);
        self.inner.client_notify.notify_one();
        Ok(())
    }

    /// Update position and front.
    pub(crate) async fn object_update(&self, id: Id, key: Key, value: Value) {
        let mut state = self.inner.client_state.lock().await;

        let Some(object) = state.objects.get_mut(&id) else {
            return;
        };

        object.props.insert(key, value);
        object.changed.insert(key);
        state.objects_changed.insert(id);

        self.inner.client_notify.notify_one();
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

    /// Create a new local object, persisting it to the database and inserting
    /// it into the in-memory client state.  Returns the new object's ID.
    pub(crate) async fn create_object(&self, ty: Type, props: Properties) -> Result<RemoteObject> {
        let id = Id::new(rand::random());

        self.db().insert_object(id, ty).await?;

        for (key, value) in props.iter() {
            self.db().set_property_value(id, key, value.clone()).await?;
        }

        let mut object = LocalObject {
            ty,
            id,
            props,
            changed: HashSet::new(),
        };

        let (sort, props) = {
            let mut state = self.inner.client_state.lock().await;

            let last = state
                .objects
                .values()
                .map(|o| o.props.get(Key::SORT).as_bytes().unwrap_or_default())
                .max();

            let sort = match last {
                Some(sort) => sorting::after(sort),
                None => object.id.to_vec(),
            };

            let sort = Value::from(sort);

            object.props.insert(Key::SORT, sort.clone());
            let props = object.props.clone();
            state.objects.insert(id, object);
            state.objects_added.insert(id);
            (sort, props)
        };

        self.db().set_property_value(id, Key::SORT, sort).await?;
        self.inner.client_notify.notify_one();

        Ok(RemoteObject { ty, id, props })
    }

    /// Delete a local object, removing it from the database and in-memory state.
    pub(crate) async fn delete_object(&self, id: Id) -> Result<()> {
        // If the mumble object is deleted, clear the mumble object setting to
        // avoid dangling references.
        if self.mumble_object() == id {
            self.store_mumble_object(Id::ZERO);
            self.set_mumblelink_transform(None).await;
        }

        self.db().delete_object(id).await?;
        let mut state = self.inner.client_state.lock().await;
        state.objects.remove(&id);
        state.objects_changed.remove(&id);
        state.objects_added.remove(&id);
        state.objects_deleted.insert(id);
        self.inner.client_notify.notify_one();
        Ok(())
    }

    pub(crate) async fn insert_image(
        &self,
        id: Id,
        content_type: ContentType,
        bytes: Vec<u8>,
        width: u32,
        height: u32,
    ) -> Result<Id> {
        let image = LocalImage {
            id,
            content_type,
            bytes,
            width,
            height,
        };

        self.db()
            .save_image(
                image.id,
                image.content_type,
                image.bytes.clone(),
                image.width,
                image.height,
            )
            .await?;

        let mut state = self.inner.client_state.lock().await;
        state.images.insert(id, image);
        state.images_added.insert(id);
        self.inner.client_notify.notify_one();
        Ok(id)
    }

    /// Delete a local image.
    pub(crate) async fn delete_image(&self, id: Id) -> Result<()> {
        self.db().delete_image(id).await?;

        let mut state = self.inner.client_state.lock().await;
        state.images.remove(&id);
        state.images_added.remove(&id);
        state.images_deleted.insert(id);
        self.inner.client_notify.notify_one();
        Ok(())
    }
}

/// An optional and atomically stored identifier.
pub struct AtomicId {
    raw: AtomicU64,
}

impl AtomicId {
    fn new(value: Id) -> Self {
        Self {
            raw: AtomicU64::new(value.get()),
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
