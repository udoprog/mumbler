use std::path::{Path, PathBuf};

/// Paths used by the application.
pub struct Paths {
    pub db: PathBuf,
}

impl Paths {
    /// Construct a new collection of paths.
    pub fn new(root: &Path) -> Self {
        Self {
            db: root.join("mumbler.sqlite"),
        }
    }
}
