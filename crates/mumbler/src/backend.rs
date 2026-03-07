use core::fmt;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use api::{Color, Id, Key, Transform, Value, Vec3};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{Mutex, MutexGuard, Notify, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::{Database, Paths};

const TRANSFORM_CHANGED: u8 = 0b0000_0001;
const LOOK_AT_CHANGED: u8 = 0b0000_0010;
const IMAGE_CHANGED: u8 = 0b0000_0100;
const COLOR_CHANGED: u8 = 0b0000_1000;
const NAME_CHANGED: u8 = 0b0001_0000;

#[derive(Debug, Clone)]
pub(crate) enum RemoteAvatarEvent {
    RemoteLost,
    Join {
        peer_id: Id,
        values: HashMap<Key, Value>,
    },
    Leave {
        peer_id: Id,
    },
    Update {
        peer_id: Id,
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
pub(crate) struct Player {
    pub(crate) transform: Transform,
    pub(crate) look_at: Option<Vec3>,
    pub(crate) image: Option<Id>,
    pub(crate) color: Color,
    pub(crate) name: Option<String>,
    changed: u8,
}

impl Player {
    /// Check if the transform has changed in the current state.
    #[inline]
    pub(crate) fn is_transform(&self) -> bool {
        self.changed & TRANSFORM_CHANGED != 0
    }

    /// Check if the look at point has changed in the current state.
    #[inline]
    pub(crate) fn is_look_at(&self) -> bool {
        self.changed & LOOK_AT_CHANGED != 0
    }

    /// Check if the player's image has changed.
    #[inline]
    pub(crate) fn is_image(&self) -> bool {
        self.changed & IMAGE_CHANGED != 0
    }

    /// Check if the player's color has changed.
    #[inline]
    pub(crate) fn is_color(&self) -> bool {
        self.changed & COLOR_CHANGED != 0
    }

    /// Check if the player's name has changed.
    #[inline]
    pub(crate) fn is_name(&self) -> bool {
        self.changed & NAME_CHANGED != 0
    }
}

/// Information about a remote peer.
#[derive(Default)]
pub(crate) struct PeerInfo {
    pub(crate) values: HashMap<Key, Value>,
}

/// Information about remote peers.
pub(crate) struct ClientState {
    pub(crate) player: Player,
    pub(crate) peers: HashMap<Id, PeerInfo>,
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

        let image = database.get::<Id>(Id::GLOBAL, Key::AVATAR_IMAGE_ID).await?;

        let color = database
            .get::<Color>(Id::GLOBAL, Key::AVATAR_COLOR)
            .await?
            .unwrap_or_else(Color::neutral);

        let transform = database
            .get::<Transform>(Id::GLOBAL, Key::AVATAR_TRANSFORM)
            .await?
            .unwrap_or_else(Transform::origin);

        let look_at = database
            .get::<Vec3>(Id::GLOBAL, Key::AVATAR_LOOK_AT)
            .await?;
        let name = database.get::<String>(Id::GLOBAL, Key::AVATAR_NAME).await?;

        Ok(Self {
            inner: Arc::new(Inner {
                database,
                paths,
                client_state: Mutex::new(ClientState {
                    player: Player {
                        transform,
                        look_at,
                        image,
                        color,
                        name,
                        changed: 0,
                    },
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
    pub(crate) async fn set_client_transform(&self, transform: Transform) {
        let mut state = self.inner.client_state.lock().await;
        state.player.transform = transform;
        state.player.changed |= TRANSFORM_CHANGED;
        self.inner.client_notify.notify_one();
    }

    /// Update the look at point.
    pub(crate) async fn set_client_look_at(&self, look_at: Option<Vec3>) {
        let mut state = self.inner.client_state.lock().await;
        state.player.look_at = look_at;
        state.player.changed |= LOOK_AT_CHANGED;
        self.inner.client_notify.notify_one();
    }

    /// Update the player's image.
    pub(crate) async fn set_client_image(&self, image: Option<Id>) {
        let mut state = self.inner.client_state.lock().await;
        state.player.image = image;
        state.player.changed |= IMAGE_CHANGED;
        self.inner.client_notify.notify_one();
    }

    /// Update the player's color.
    pub(crate) async fn set_client_color(&self, color: Color) {
        let mut state = self.inner.client_state.lock().await;
        state.player.color = color;
        state.player.changed |= COLOR_CHANGED;
        self.inner.client_notify.notify_one();
    }

    /// Update the player's display name.
    pub(crate) async fn set_client_name(&self, name: Option<String>) {
        let mut state = self.inner.client_state.lock().await;
        state.player.name = name;
        state.player.changed |= NAME_CHANGED;
        self.inner.client_notify.notify_one();
    }

    /// Get the current state, resetting any changed flags.
    pub(crate) async fn take_client_player(&self) -> Player {
        let mut state = self.inner.client_state.lock().await;
        let out = state.player.clone();
        state.player.changed = 0;
        out
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
