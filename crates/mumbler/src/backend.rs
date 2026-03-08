use core::fmt;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use api::{Id, Key, PeerId, RemoteObject, Transform, Value};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{Mutex, MutexGuard, Notify, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::{Database, Paths};

#[derive(Debug, Clone)]
pub(crate) enum RemoteAvatarEvent {
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
}

#[derive(Debug, Clone)]
pub(crate) enum BackendEvent {
    RemoteAvatar(RemoteAvatarEvent),
    Notification {
        error: bool,
        component: String,
        message: String,
    },
}

/// State for the backend.
#[derive(Debug, Clone)]
pub(crate) struct LocalObject {
    pub(crate) properties: HashMap<Key, Value>,
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
    pub(crate) transform: Transform,
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

        let mut objects = HashMap::new();

        for id in database.objects(api::Type::AVATAR).await? {
            let mut values = HashMap::new();

            for (key, value) in database.properties(id).await? {
                values.insert(key, value);
            }

            objects.insert(
                id,
                LocalObject {
                    properties: values,
                    changed: HashSet::new(),
                },
            );
        }

        Ok(Self {
            inner: Arc::new(Inner {
                database,
                paths,
                client_state: Mutex::new(ClientState {
                    objects,
                    objects_changed: HashSet::new(),
                    peers: HashMap::new(),
                }),
                images: RwLock::new(Images {
                    images: HashMap::new(),
                }),
                client_notify: Notify::new(),
                client_restart_notify: Notify::new(),
                mumblelink_state: Mutex::new(MumblelinkState {
                    transform: Transform::origin(),
                }),
                mumblelink_notify: Notify::new(),
                mumblelink_restart_notify: Notify::new(),
                broadcast,
            }),
        })
    }

    /// Set up an event subscriber.
    pub(crate) fn subscribe(&self) -> Receiver<BackendEvent> {
        self.inner.broadcast.subscribe()
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
    pub(crate) async fn set_mumblelink_transform(&self, transform: Transform) {
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

    /// Get a reference to the database.
    pub(crate) fn db(&self) -> &Database {
        &self.inner.database
    }
}
