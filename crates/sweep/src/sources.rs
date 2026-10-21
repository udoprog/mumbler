use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

/// Every file under `dir` named with `extension`, as `(path, text)`, sorted so
/// that a report reads the same twice. `deep` walks the directories below it
/// too, which is how the sources are laid out and the stylesheets are not.
pub(crate) fn files(dir: &Path, extension: &str, deep: bool) -> Result<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    read(dir, extension, deep, &mut out)?;
    out.sort_by(|(a, _), (b, _)| a.cmp(b));
    Ok(out)
}

/// Where a file is, said the short way: relative to the directory above the one
/// walked, so that a finding reads `src/ui/mix.rs` whether the sweep was started
/// from the crate or from the workspace root.
pub(crate) fn at(path: &Path, dir: &Path) -> String {
    dir.parent()
        .and_then(|base| path.strip_prefix(base).ok())
        .unwrap_or(path)
        .display()
        .to_string()
}

fn read(dir: &Path, extension: &str, deep: bool, into: &mut Vec<(PathBuf, String)>) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| anyhow!("Reading {}", dir.display()))?;

    for entry in entries {
        let path = entry
            .with_context(|| anyhow!("Reading an entry of {}", dir.display()))?
            .path();

        if path.is_dir() {
            if deep {
                read(&path, extension, deep, into)?;
            }

            continue;
        }

        if path.extension().is_none_or(|e| e != extension) {
            continue;
        }

        let text =
            fs::read_to_string(&path).with_context(|| anyhow!("Reading {}", path.display()))?;

        into.push((path, text));
    }

    Ok(())
}
