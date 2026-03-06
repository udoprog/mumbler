use std::sync::Arc;

use api::Vec3;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::{Database, Paths};

#[derive(Debug)]
pub enum Event {
    Move(Vec3, Vec3),
}

/// The backend of the application, containing the database and other shared state.
#[derive(Clone)]
pub struct Backend {
    database: Database,
    sender: UnboundedSender<Event>,
    receiver: Arc<Mutex<UnboundedReceiver<Event>>>,
    #[allow(unused)]
    paths: Arc<Paths>,
}

impl Backend {
    /// Construct a new backend.
    pub fn new(database: Database, paths: Arc<Paths>) -> Self {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        Self {
            database,
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
            paths,
        }
    }

    /// Send an event.
    pub(crate) fn send(&self, event: Event) {
        _ = self.sender.send(event);
    }

    /// Receive the next event.
    pub(crate) async fn event(&self) -> Option<Event> {
        let mut receiver = self.receiver.lock().await;
        receiver.recv().await
    }

    /// Get a reference to the database.
    pub(crate) fn db(&self) -> &Database {
        &self.database
    }
}
