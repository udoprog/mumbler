//! Checks that the frontend's localized strings and its Fluent catalog
//! agree, both ways: every key the code asks for is defined in the catalog,
//! and every key the catalog defines is asked for somewhere (or is one a
//! library's catalog defines, which the application's may override).
//!
//! [`check_l10n`] walks a source directory for the keys it names, hands them
//! with the catalog to phrasebook's `check::coverage`, and reports what it
//! found as [`Strings`]; it never panics. It is what
//! `cargo run -p tools -- check-l10n` prints and what `cargo test -p web`
//! asserts. A sweep that finds implausibly few keys or asks is an error
//! rather than a pass, so a scanner that stopped matching cannot report a
//! clean result.

mod l10n;
mod sources;

pub use self::l10n::{Strings, check_l10n, read_catalog};
