use super::{Database, Paths};

/// The backend of the application, containing the database and other shared state.
pub struct Backend {
    database: Database,
    #[allow(unused)]
    paths: Paths,
}

impl Backend {
    /// Construct a new backend.
    pub fn new(database: Database, paths: Paths) -> Self {
        Self { database, paths }
    }

    /// Get a reference to the database.
    pub(crate) fn db(&self) -> &Database {
        &self.database
    }
}
