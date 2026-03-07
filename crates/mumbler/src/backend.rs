use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use api::{Color, Id, Transform, Vec3};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{Mutex, MutexGuard, Notify, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::{Database, Paths};

const TRANSFORM_CHANGED: u8 = 0b0000_0001;
const IMAGE_CHANGED: u8 = 0b0000_0010;
const COLOR_CHANGED: u8 = 0b0000_0100;

#[derive(Debug, Clone)]
pub(crate) enum BackendEvent {
    RemoteLost,
    Join { peer_id: Id },
    Leave { peer_id: Id },
    Moved { peer_id: Id, transform: Transform },
    ImageUpdated { peer_id: Id, image: Option<Id> },
    ColorUpdated { peer_id: Id, color: Color },
}

/// State for the backend.
#[derive(Debug, Clone)]
pub(crate) struct Player {
    pub(crate) transform: Transform,
    pub(crate) look_at: Option<Vec3>,
    pub(crate) image: Option<Id>,
    pub(crate) color: Color,
    changed: u8,
}

impl Player {
    /// Check if the transform has changed in the current state.
    #[inline]
    pub(crate) fn is_transform(&self) -> bool {
        self.changed & TRANSFORM_CHANGED != 0
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
}

/// Information about a remote peer.
pub(crate) struct PeerInfo {
    pub(crate) transform: Transform,
    pub(crate) image: Option<Id>,
    pub(crate) color: Color,
}

impl Default for PeerInfo {
    fn default() -> Self {
        Self {
            transform: Transform::origin(),
            image: None,
            color: Color::neutral(),
        }
    }
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
    pub(crate) enabled: bool,
    pub(crate) restart: bool,
}

struct Inner {
    database: Database,
    #[allow(unused)]
    paths: Paths,
    client_state: Mutex<ClientState>,
    client_notify: Notify,
    mumblelink_state: Mutex<MumblelinkState>,
    mumblelink_notify: Notify,
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

        let image = database.get_config::<Id>("avatar/image").await?;

        let color = database
            .get_config::<Color>("avatar/color")
            .await?
            .unwrap_or_else(Color::neutral);

        let transform = database
            .get_config::<Transform>("avatar/transform")
            .await?
            .unwrap_or_else(Transform::origin);

        let look_at = database.get_config::<Vec3>("avatar/look-at").await?;

        let mumblelink_enabled = database
            .get_config::<bool>("mumble/enabled")
            .await?
            .unwrap_or_default();

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
                        changed: 0,
                    },
                    peers: HashMap::new(),
                }),
                images: RwLock::new(Images {
                    images: HashMap::new(),
                }),
                client_notify: Notify::new(),
                mumblelink_state: Mutex::new(MumblelinkState {
                    transform: Transform::origin(),
                    restart: false,
                    enabled: mumblelink_enabled,
                }),
                mumblelink_notify: Notify::new(),
                broadcast,
            }),
        })
    }

    /// Set up an event subscriber.
    pub(crate) fn subscribe(&self) -> Receiver<BackendEvent> {
        self.inner.broadcast.subscribe()
    }

    /// Lock the remote state and access it.
    pub(crate) async fn state(&self) -> MutexGuard<'_, ClientState> {
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

    /// Receive the next event.
    pub(crate) async fn client_wait(&self) {
        self.inner.client_notify.notified().await;
    }

    /// Receive the next transform update.
    pub(crate) async fn mumblelink_wait(&self) {
        self.inner.mumblelink_notify.notified().await;
    }

    /// Update position and front.
    pub(crate) async fn set_client_transform(&self, transform: Transform, look_at: Option<Vec3>) {
        let mut state = self.inner.client_state.lock().await;
        state.player.transform = transform;
        state.player.look_at = look_at;
        state.player.changed |= TRANSFORM_CHANGED;
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
    pub(crate) async fn restart_mumblelink(&self) {
        let mut state = self.inner.mumblelink_state.lock().await;
        state.restart = true;
        self.inner.mumblelink_notify.notify_one();
    }

    /// Set whether mumblelink is enabled.
    pub(crate) async fn set_mumblelink_enabled(&self, enabled: bool) {
        let mut state = self.inner.mumblelink_state.lock().await;
        state.enabled = enabled;
        self.inner.mumblelink_notify.notify_one();
    }

    /// Get a reference to the database.
    pub(crate) fn db(&self) -> &Database {
        &self.inner.database
    }
}
