use wasm_bindgen::JsCast;
use web_sys::HtmlSelectElement;
use yew::prelude::*;

use crate::l10n::{self, SetLang};

/// Picks the language of the interface, among those the phrasebook ships.
/// The names are endonyms, so they read the same in any language.
#[component(LanguagePicker)]
pub(crate) fn language_picker() -> Html {
    let l10n = l10n::use_phrases();
    let set = use_context::<SetLang>();
    let current = l10n.lang();

    let onchange = {
        let l10n = l10n.clone();

        Callback::from(move |e: Event| {
            let Some(select) = e
                .target()
                .and_then(|t| t.dyn_into::<HtmlSelectElement>().ok())
            else {
                return;
            };

            let value = select.value();

            let Some(lang) = l10n
                .languages()
                .iter()
                .map(|language| language.lang())
                .find(|lang| lang.to_string() == value)
            else {
                return;
            };

            if let Some(SetLang(set)) = &set {
                set.emit(lang);
            }
        })
    };

    html! {
        <section class="input-group">
            <label for="language">{l10n.text("settings-language")}</label>

            <select id="language" {onchange}>
                {for l10n.languages().iter().map(|language| {
                    let lang = language.lang();
                    let value = lang.to_string();
                    let tag = value.clone();

                    html! {
                        <option {value} lang={tag} selected={lang == current}>
                            {language.name()}
                        </option>
                    }
                })}
            </select>
        </section>
    }
}
