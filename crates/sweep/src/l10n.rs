//! The string sweep, which is a closed pair rather than one check: every key
//! the code asks for is in the catalog, and every key in the catalog is one
//! something asks for. Half a rename fails one of the two.
//!
//! The sources are read as text, for the keys they spell; the catalog's ids
//! and the verdict are phrasebook's, through `check::coverage`. web's
//! `l10n::tests` hold the other side: `the_catalog_is_sound` checks that
//! every id formats and none is defined twice.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use phrasebook::{Catalog, Lang, Source, check};

use crate::sources;

/// Fewer keys or asks than this means the sweep stopped matching.
const PLAUSIBLE: usize = 50;

/// The calls that name a key as a literal.
/// The last also matches `html_around(&self.l10n, "...", ..)`.
const CALLS: [&str; 3] = ["l10n.text(\"", "l10n.msg(\"", "l10n, \""];

/// What the string sweep found.
pub struct Strings {
    /// How many keys the catalog defines.
    pub defined: usize,
    /// How many literal asks the sources make, the same key counted once per
    /// place it is asked.
    pub asked: usize,
    /// Keys asked for that neither the catalog nor a library defines, with
    /// where they were asked.
    pub missing: Vec<(String, String)>,
    /// Keys the catalog defines that nothing asks for.
    pub dangling: Vec<String>,
    /// Anything else phrasebook's coverage check reported, such as the
    /// catalog having no source for the language, in its words.
    pub other: Vec<String>,
}

impl Strings {
    /// Whether the sweep found nothing to answer for, in either direction.
    pub fn is_clean(&self) -> bool {
        self.missing.is_empty() && self.dangling.is_empty() && self.other.is_empty()
    }
}

/// A catalog of one English source read from `ftl`, for a sweep of a file
/// rather than of a catalog compiled in. It is leaked, as phrasebook's
/// catalogs are `'static`; a sweep runs once.
pub fn read_catalog(name: &'static str, ftl: &Path) -> Result<&'static Catalog> {
    let text =
        std::fs::read_to_string(ftl).with_context(|| anyhow!("Reading {}", ftl.display()))?;
    let text: &'static str = Box::leak(text.into_boxed_str());
    let sources: &'static [Source] = Box::leak(Box::new([Source::new(Lang::EN_US, text)]));
    Ok(Box::leak(Box::new(Catalog::new(name, sources))))
}

/// Sweep `catalog` in `lang` against the sources under `src_dir`, both ways.
///
/// `libraries` are the catalogs the application's phrasebook takes after
/// its own (watchword's), whose components ask for their ids. The catalog
/// may define those too, to word them in its own way or language, so one of
/// them counts as asked for, and an asked id one of them defines is not
/// missing. An id no library has is swept like any other.
pub fn check_l10n(
    catalog: &Catalog,
    lang: Lang,
    src_dir: &Path,
    libraries: &[&Catalog],
) -> Result<Strings> {
    let names = check::ids(catalog, lang);

    if names.len() < PLAUSIBLE {
        bail!(
            "the sweep found {} keys, which means it stopped working rather \
             than that the application stopped having any",
            names.len(),
        );
    }

    let files = sources::files(src_dir, "rs", true)?;
    let asked = asked_in(&files, src_dir, &CALLS);

    if asked.len() < PLAUSIBLE {
        bail!(
            "the sweep found {} asks, which means it stopped working rather \
             than that the application stopped asking for any",
            asked.len(),
        );
    }

    let spelled = |key: &str| -> bool {
        let literal = format!("\"{key}\"");
        files.iter().any(|(_, text)| text.contains(&literal))
    };

    // Keys the sources reach without a literal call: spelled out elsewhere,
    // such as a key chosen in a `match`.
    let reached = names.iter().filter(|key| spelled(key));

    let ids = asked
        .iter()
        .map(|(key, _)| key.as_str())
        .chain(reached.map(String::as_str));

    let mut missing = Vec::new();
    let mut dangling = Vec::new();
    let mut other = Vec::new();

    if let Err(problems) = check::coverage(catalog, lang, ids, libraries) {
        for problem in &problems {
            match Finding::of(problem.message()) {
                Some(Finding::Missing(key)) => {
                    missing.extend(asked.iter().filter(|(asked, _)| asked == key).cloned())
                }
                Some(Finding::Dangling(key)) => dangling.push(key.to_owned()),
                None => other.push(problem.to_string()),
            }
        }
    }

    Ok(Strings {
        defined: names.len(),
        asked: asked.len(),
        missing,
        dangling,
        other,
    })
}

/// One of `check::coverage`'s two findings, read from its message.
enum Finding<'a> {
    Missing(&'a str),
    Dangling(&'a str),
}

impl<'a> Finding<'a> {
    fn of(message: &'a str) -> Option<Self> {
        let rest = message.strip_prefix('`')?;
        let (key, rest) = rest.split_once('`')?;

        match rest {
            " is asked for but not defined" => Some(Finding::Missing(key)),
            " is defined but never asked for" => Some(Finding::Dangling(key)),
            _ => None,
        }
    }
}

fn asked_in(files: &[(PathBuf, String)], src_dir: &Path, calls: &[&str]) -> Vec<(String, String)> {
    let mut asked = Vec::new();

    for (path, source) in files {
        let at = sources::at(path, src_dir);

        for call in calls {
            for (index, _) in source.match_indices(call) {
                let rest = &source[index + call.len()..];

                let Some(end) = rest.find('"') else {
                    continue;
                };

                asked.push((rest[..end].to_owned(), at.clone()));
            }
        }
    }

    asked
}

#[cfg(test)]
mod tests {
    use phrasebook::{Catalog, Lang, Source, check};

    use super::Finding;

    static FIXTURE: Catalog = Catalog::new("fixture", &[Source::new(Lang::EN_US, "kept = K\n")]);

    /// **The sweep reads both of phrasebook's coverage findings**, as
    /// phrasebook words them, so a reworded one fails here rather than
    /// passing unread.
    #[test]
    fn coverage_findings_are_read() {
        let problems = check::coverage(&FIXTURE, Lang::EN_US, ["asked"], &[]).unwrap_err();
        let findings = problems
            .iter()
            .map(|problem| match Finding::of(problem.message()) {
                Some(Finding::Missing(key)) => format!("missing {key}"),
                Some(Finding::Dangling(key)) => format!("dangling {key}"),
                None => format!("other {problem}"),
            })
            .collect::<Vec<_>>();

        assert_eq!(findings, ["missing asked", "dangling kept"]);
        assert!(Finding::of("no source for this language").is_none());
    }
}
