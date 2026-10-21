use std::time::Duration;

use auth::yew023::AuthProvider;
use gloo::net::http::Request;
use yew::platform::spawn_local;
use yew::platform::time::sleep;
use yew::prelude::*;

use crate::l10n::{self, Phrases, SetLang};

use super::App;

/// Whether the server requires signing in, as `/api/access` says: only when
/// it is bound to an address other than loopback.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Access {
    Open,
    SignIn,
}

/// Provides the interface's [`Phrases`] and the language picker's
/// [`SetLang`], and renders [`App`], behind watchword's sign-in when the
/// server requires it.
#[component(Root)]
pub fn root() -> Html {
    let l10n = use_state(l10n::initial);
    let access = use_state(|| None::<Access>);

    let set_lang = {
        let l10n = l10n.clone();

        SetLang(Callback::from(move |lang| {
            if l10n.lang() == lang {
                return;
            }

            let phrases = l10n.with_lang(lang);
            phrasebook::browser::set_document_lang(lang);
            phrases.store();
            l10n.set(phrases);
        }))
    };

    {
        let access = access.clone();

        use_effect_with((), move |_| {
            spawn_local(async move {
                loop {
                    match fetch_access().await {
                        Ok(found) => {
                            access.set(Some(found));
                            break;
                        }
                        Err(error) => {
                            tracing::warn!(%error, "asking whether sign-in is required");
                            sleep(Duration::from_secs(2)).await;
                        }
                    }
                }
            });
        });
    }

    let content = match *access {
        None => html! {
            <div id="content" class="container">{l10n.text("app-connecting")}</div>
        },
        Some(Access::Open) => html! {
            <App />
        },
        Some(Access::SignIn) => html! {
            <AuthProvider title={l10n.text("app-title")}>
                <App />
            </AuthProvider>
        },
    };

    html! {
        <ContextProvider<Phrases> context={(*l10n).clone()}>
            <ContextProvider<SetLang> context={set_lang}>
                {content}
            </ContextProvider<SetLang>>
        </ContextProvider<Phrases>>
    }
}

async fn fetch_access() -> Result<Access, gloo::net::Error> {
    let response = Request::get("/api/access").send().await?;

    if !response.ok() {
        return Err(gloo::net::Error::GlooError(format!(
            "status {}",
            response.status()
        )));
    }

    // A server from before sign-in existed answers with the page itself.
    match response.text().await?.as_str() {
        "sign-in" => Ok(Access::SignIn),
        _ => Ok(Access::Open),
    }
}
