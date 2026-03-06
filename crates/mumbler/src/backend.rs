use std::collections::HashMap;
use std::sync::Arc;

use api::{Id, Vec3};
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::{Mutex, MutexGuard, Notify, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::{Database, Paths};

const TRANSLATION_CHANGED: u8 = 0b0000_0001;
const IMAGE_CHANGED: u8 = 0b0000_0010;

#[derive(Debug, Clone, Copy)]
pub(crate) enum BackendEvent {
    RemoteLost,
    Join {
        peer_id: Id,
    },
    Leave {
        peer_id: Id,
    },
    Moved {
        peer_id: Id,
        position: Vec3,
        front: Vec3,
    },
    ImageUpdated {
        peer_id: Id,
        image: Option<Id>,
    },
}

/// State for the backend.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Player {
    pub(crate) position: Vec3,
    pub(crate) front: Vec3,
    pub(crate) image: Option<Id>,
    changed: u8,
}

impl Player {
    /// Check if translation has changed in the current state.
    #[inline]
    pub(crate) fn is_translated(&self) -> bool {
        self.changed & TRANSLATION_CHANGED != 0
    }

    /// Check if the player's image has changed.
    #[inline]
    pub(crate) fn is_image(&self) -> bool {
        self.changed & IMAGE_CHANGED != 0
    }
}

/// Information about a remote peer.
pub(crate) struct PeerInfo {
    pub(crate) position: Vec3,
    pub(crate) front: Vec3,
    pub(crate) image: Option<Id>,
}

impl Default for PeerInfo {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            front: Vec3::FORWARD,
            image: None,
        }
    }
}

/// Information about remote peers.
pub(crate) struct State {
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

struct Inner {
    database: Database,
    #[allow(unused)]
    paths: Paths,
    state: Mutex<State>,
    images: RwLock<Images>,
    notify: Notify,
    broadcast: Sender<BackendEvent>,
}

/// The backend of the application, containing the database and other shared state.
#[derive(Clone)]
pub struct Backend {
    inner: Arc<Inner>,
}

impl Backend {
    /// Construct a new backend.
    pub async fn new(database: Database, paths: Paths) -> Self {
        let (broadcast, _) = tokio::sync::broadcast::channel(16);

        let image = database
            .get_config::<Id>("avatar/image")
            .await
            .unwrap_or(None);

        Self {
            inner: Arc::new(Inner {
                database,
                paths,
                state: Mutex::new(State {
                    player: Player {
                        position: Vec3::ZERO,
                        front: Vec3::FORWARD,
                        image,
                        changed: 0,
                    },
                    peers: HashMap::new(),
                }),
                images: RwLock::new(Images {
                    images: HashMap::new(),
                }),
                notify: Notify::new(),
                broadcast,
            }),
        }
    }

    /// Set up an event subscriber.
    pub(crate) fn subscribe(&self) -> Receiver<BackendEvent> {
        self.inner.broadcast.subscribe()
    }

    /// Lock the remote state and access it.
    pub(crate) async fn state(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().await
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

    /// Update position and front.
    pub(crate) async fn set_position_front(&self, position: Vec3, front: Vec3) {
        let mut state = self.inner.state.lock().await;
        state.player.position = position;
        state.player.front = front;
        state.player.changed |= TRANSLATION_CHANGED;
        self.inner.notify.notify_one();
    }

    /// Update the player's image.
    pub(crate) async fn set_image(&self, image: Option<Id>) {
        let mut state = self.inner.state.lock().await;
        state.player.image = image;
        state.player.changed |= IMAGE_CHANGED;
        self.inner.notify.notify_one();
    }

    /// Get the current state, resetting any changed flags.
    pub(crate) async fn take_player(&self) -> Player {
        let mut state = self.inner.state.lock().await;
        let out = state.player;
        state.player.changed = 0;
        out
    }

    /// Receive the next event.
    pub(crate) async fn wait(&self) {
        self.inner.notify.notified().await;
    }

    /// Get a reference to the database.
    pub(crate) fn db(&self) -> &Database {
        &self.inner.database
    }
}
