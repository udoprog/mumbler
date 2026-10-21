//! mumbler's checks. Run from anywhere in the workspace with
//! `cargo run -p tools -- <command>`:
//!
//! - `check-l10n` reports every localized key the frontend asks for that
//!   `crates/web/locales/en-US.ftl` lacks, and every key it defines that
//!   nothing asks for. It writes nothing and fails when it finds either. A
//!   key of watchword's catalog (`watchword-*`, which the frontend's
//!   phrasebook takes after its own) is asked for by watchword's components,
//!   so mumbler's file may define it to override watchword's wording.

use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const WEB_LOCALE: &str = "crates/web/locales/en-US.ftl";
const WEB_SRC_DIR: &str = "crates/web/src";

/// The catalogs the frontend's phrasebook takes after its own: watchword's,
/// as `crates/web/src/l10n.rs` registers it.
static LIBRARIES: &[&phrasebook::Catalog] = &[&auth::phrasebookcatalog01::L10N];

fn main() -> Result<()> {
    let root = root()?;

    match env::args().nth(1).as_deref() {
        Some("check-l10n") => check_l10n(&root),
        Some(command) => bail!("unknown command `{command}`, expected `check-l10n`"),
        None => bail!("expected a command: `check-l10n`"),
    }
}

/// The workspace root, two directories above this crate.
fn root() -> Result<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));

    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("the workspace root")
}

/// The string sweep, both directions at once.
fn check_l10n(root: &Path) -> Result<()> {
    let catalog = sweep::read_catalog("mumbler", &root.join(WEB_LOCALE))?;
    let report = sweep::check_l10n(
        catalog,
        phrasebook::Lang::EN_US,
        &root.join(WEB_SRC_DIR),
        LIBRARIES,
    )?;

    println!(
        "{} keys defined, asked for in {} places.",
        report.defined, report.asked
    );

    for (key, at) in &report.missing {
        println!("{at}: {key} is asked for and the catalog has not got it");
    }

    for key in &report.dangling {
        println!("{WEB_LOCALE}: {key} is defined and nothing asks for it");
    }

    for problem in &report.other {
        println!("{WEB_LOCALE}: {problem}");
    }

    if !report.is_clean() {
        bail!(
            "{} keys missing, {} with nothing asking for them and {} other problems.",
            report.missing.len(),
            report.dangling.len(),
            report.other.len()
        );
    }

    println!("Nothing dangling.");
    Ok(())
}
