//! User-facing text, from the Fluent files in `crates/web/locales`.
//!
//! Every string a user reads comes through [`Phrases`], which [`Root`]
//! provides as a yew context: function components read it with
//! [`use_phrases`], struct components keep a [`Subscription`]. The chosen
//! language is kept in localStorage under phrasebook's default key, and
//! [`Root`] also provides a [`SetLang`] callback that switches it, which the
//! language picker on the settings page uses.
//!
//! The sign-in, registration, account and user pages are watchword's
//! components, whose text is watchword's own catalog
//! ([`auth::yew023::L10N`], ids under `watchword-`). It is added after
//! mumbler's, so the components follow the chosen language too, and a
//! `watchword-*` id in mumbler's files takes precedence over watchword's.
//!
//! `cargo run -p tools -- check-l10n` and the tests below check, through
//! phrasebook's `check::coverage`, that every key the code asks for is in the
//! catalog and every key in the catalog is asked for.
//!
//! [`Root`]: crate::components::Root

use std::ops::Deref;

use phrasebook::yew023::PhrasesContext;
use phrasebook::{Catalog, Lang, Phrasebook, Source};
use yew::{Callback, Component, Context};

pub(crate) use phrasebook::yew023::use_phrases;
pub(crate) use phrasebook::{Phrases, args};

static L10N: Catalog = Catalog::new(
    "mumbler",
    &[Source::new(
        Lang::EN_US,
        include_str!("../locales/en-US.ftl"),
    )],
);

/// Every shipped language, over mumbler's catalog and then watchword's,
/// falling back to the first.
pub(crate) fn phrasebook() -> Phrasebook {
    Phrasebook::builder()
        .language(Lang::EN_US, "English")
        .catalog(&L10N)
        .catalog(&auth::yew023::L10N)
        .build()
}

/// The language to start in: a previous choice if there is one, otherwise the
/// browser's preference, otherwise the fallback.
pub(crate) fn initial() -> Phrases {
    phrasebook::browser::initial(&phrasebook())
}

/// Switches the language of every component and remembers the choice.
#[derive(Clone, PartialEq)]
pub(crate) struct SetLang(pub(crate) Callback<Lang>);

/// The [`Phrases`] a struct component renders with, kept up to date.
///
/// A struct component subscribes once, in `create`, with a message of its own
/// that carries the new handle, and hands that message to
/// [`Subscription::update`], so that a language change renders it again.
/// Without a provided handle it reads phrasebook's default.
pub(crate) struct Subscription(PhrasesContext);

impl Subscription {
    /// Subscribe `ctx`'s component to the provided [`Phrases`], sent back to
    /// it as `msg`.
    pub(crate) fn new<C, M>(ctx: &Context<C>, msg: M) -> Self
    where
        C: Component,
        M: Fn(Phrases) -> C::Message + 'static,
    {
        Self(PhrasesContext::new(ctx.link(), ctx.link().callback(msg)))
    }

    /// Take the newly provided handle; whether the component has to render
    /// again.
    pub(crate) fn update(&mut self, phrases: Phrases) -> bool {
        self.0.set(phrases)
    }
}

impl Deref for Subscription {
    type Target = Phrases;

    fn deref(&self) -> &Phrases {
        self.0.phrases()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use phrasebook::check;

    use super::*;

    #[test]
    fn the_catalog_is_sound() {
        check::all(&L10N, &[&auth::yew023::L10N]).unwrap();
    }

    #[test]
    fn the_catalogs_are_in_the_phrasebook() {
        let book = phrasebook();
        assert!(book.has_catalog(&L10N));
        assert!(book.has_catalog(&auth::yew023::L10N));
        assert_eq!(book.fallback(), Lang::EN_US);
    }

    #[test]
    fn a_preference_matches_on_the_primary_subtag() {
        let book = phrasebook();
        assert_eq!(book.negotiate(["en-GB"]), Lang::EN_US);
        assert_eq!(book.negotiate(["sv-SE"]), Lang::EN_US);
    }

    #[test]
    fn watchword_messages_resolve() {
        let phrases = phrasebook().phrases(Lang::EN_US);
        assert_ne!(&*phrases.text("watchword-sign-in"), "watchword-sign-in");
    }

    /// Every key the code asks for is in the catalog, and every key in the
    /// catalog is asked for.
    #[test]
    fn the_sweep_is_clean() {
        let web = Path::new(env!("CARGO_MANIFEST_DIR"));
        let strings =
            sweep::check_l10n(&L10N, Lang::EN_US, &web.join("src"), &[&auth::yew023::L10N])
                .expect("a sweep of the keys");

        let mut findings = Vec::new();

        for (key, at) in &strings.missing {
            findings.push(format!(
                "{at}: {key} is asked for and the catalog has not got it"
            ));
        }

        for key in &strings.dangling {
            findings.push(format!("{key} is defined and nothing asks for it"));
        }

        findings.extend(strings.other.iter().cloned());
        assert!(findings.is_empty(), "{findings:#?}");
    }
}
