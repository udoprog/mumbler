use core::fmt;
use core::sync::atomic::Ordering;

use core::sync::atomic::AtomicU64;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use api::Properties;
use api::{Id, Key, PeerId, RemoteObject, Transform, Type, Value};
use parking_lot::RwLock as BlockingRwLock;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{Mutex, MutexGuard, Notify, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::{Database, Paths};

#[derive(Debug, Clone)]
pub(crate) struct LocalConfigEvent {
    pub(crate) body: api::ConfigUpdateBody,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalUpdateEvent {
    pub(crate) body: api::LocalUpdateBody,
}

#[derive(Debug, Clone)]
pub(crate) enum RemoteUpdateEvent {
    RemoteLost,
    Join {
        peer_id: PeerId,
        objects: Vec<RemoteObject>,
    },
    Leave {
        peer_id: PeerId,
    },
    Update {
        peer_id: PeerId,
        object_id: Id,
        key: Key,
        value: Value,
    },
    ObjectAdded {
        peer_id: PeerId,
        object: RemoteObject,
    },
    ObjectRemoved {
        peer_id: PeerId,
        object_id: Id,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum BackendEvent {
    ConfigUpdate(LocalConfigEvent),
    LocalUpdate(LocalUpdateEvent),
    RemoteUpdate(RemoteUpdateEvent),
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
    pub(crate) group_id: Option<Id>,
    pub(crate) properties: Properties,
    pub(crate) changed: HashSet<Key>,
}

#[derive(Default)]
pub(crate) struct PeerInfo {
    pub(crate) objects: HashMap<Id, RemoteObject>,
}

/// Information about remote peers.
pub(crate) struct ClientState {
    /// Local objects.
    pub(crate) objects: HashMap<Id, LocalObject>,
    /// Identifiers of objects that have been changed.
    pub(crate) objects_changed: HashSet<Id>,
    /// Identifiers of objects that have been added.
    pub(crate) object_added: HashSet<Id>,
    /// Identifiers of objects that have been deleted.
    pub(crate) object_deleted: HashSet<Id>,
    /// Remote objects.
    pub(crate) peers: HashMap<PeerId, PeerInfo>,
}

/// Temporary images.
pub(crate) struct Images {
    images: HashMap<Id, Vec<u8>>,
}

impl Images {
    /// Store an image and return its ID.
    pub(crate) fn store(&mut self, data: Vec<u8>) -> Id {
        let id = Id::new(rand::random());
        self.images.insert(id, data);
        id
    }

    /// Remove an image by ID.
    pub(crate) fn remove(&mut self, id: Id) {
        self.images.remove(&id);
    }

    /// Get an image by ID.
    pub(crate) fn get(&self, id: &Id) -> Option<&[u8]> {
        Some(self.images.get(id)?.as_slice())
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
    pub async fn new(database: Database, paths: Paths) -> Result<Self> {
        let (broadcast, _) = tokio::sync::broadcast::channel(16);

        tracing::debug!("Loading objects from database");

        let mut objects = HashMap::new();
        let mut hidden = HashSet::new();

        for (id, ty, group_id) in database.objects().await? {
            tracing::debug!(?id, ?ty, "Loading object");

            let mut properties = Properties::new();

            for (key, value) in database.properties(id).await? {
                tracing::debug!(?id, ?key, ?value, "Loading property");

                match key {
                    Key::HIDDEN => {
                        if value.as_bool().unwrap_or_default() {
                            hidden.insert(id);
                        }
                    }
                    _ => {}
                }

                properties.insert(key, value);
            }

            objects.insert(
                id,
                LocalObject {
                    ty,
                    group_id,
                    properties,
                    changed: HashSet::new(),
                },
            );
        }

        let mumble_object = database.config(Key::MUMBLE_OBJECT).await?;

        tracing::debug!("Loaded {} objects", objects.len());

        Ok(Self {
            inner: Arc::new(Inner {
                database,
                paths,
                client_state: Mutex::new(ClientState {
                    objects,
                    objects_changed: HashSet::new(),
                    object_added: HashSet::new(),
                    object_deleted: HashSet::new(),
                    peers: HashMap::new(),
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
    pub(crate) fn mumble_object(&self) -> Option<Id> {
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
    pub(crate) fn store_mumble_object(&self, id: Option<Id>) {
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

    /// Update position and front.
    pub(crate) async fn set_client(&self, id: Id, key: Key, value: Value) {
        let mut state = self.inner.client_state.lock().await;

        let Some(object) = state.objects.get_mut(&id) else {
            return;
        };

        object.properties.insert(key, value);
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
    pub(crate) async fn create_object(
        &self,
        ty: Type,
        properties: Properties,
    ) -> Result<RemoteObject> {
        let id = Id::new(rand::random());

        self.db().insert_object(id, ty).await?;

        for (key, value) in properties.iter() {
            self.db().set_property_value(id, key, value.clone()).await?;
        }

        let mut state = self.inner.client_state.lock().await;

        state.objects.insert(
            id,
            LocalObject {
                ty,
                group_id: None,
                properties: properties.clone(),
                changed: HashSet::new(),
            },
        );

        state.object_added.insert(id);
        self.inner.client_notify.notify_one();

        Ok(RemoteObject {
            ty,
            id,
            group_id: None,
            properties: properties.clone(),
        })
    }

    /// Delete a local object, removing it from the database and in-memory state.
    pub(crate) async fn delete_object(&self, id: Id) -> Result<()> {
        // If the mumble object is deleted, clear the mumble object setting to
        // avoid dangling references.
        if self.mumble_object() == Some(id) {
            self.store_mumble_object(None);
            self.set_mumblelink_transform(None).await;
        }

        self.db().delete_object(id).await?;
        let mut state = self.inner.client_state.lock().await;
        state.objects.remove(&id);
        state.objects_changed.remove(&id);
        state.object_added.remove(&id);
        state.object_deleted.insert(id);
        self.inner.client_notify.notify_one();
        Ok(())
    }
}

/// An optional and atomically stored identifier.
pub struct AtomicId {
    raw: AtomicU64,
}

impl AtomicId {
    fn new(value: Option<Id>) -> Self {
        Self {
            raw: AtomicU64::new(value.map_or(u64::MAX, |id| id.get())),
        }
    }

    /// Load the identifier from the atomic, returning None if it is invalid.
    pub fn load(&self) -> Option<Id> {
        let id = self.raw.load(Ordering::Relaxed);

        if id == u64::MAX {
            None
        } else {
            Some(Id::new(id))
        }
    }

    /// Store an identifier in the atomic, replacing any existing value.
    pub fn store(&self, id: Option<Id>) {
        self.raw
            .store(id.map_or(u64::MAX, |id| id.get()), Ordering::Relaxed);
    }
}
