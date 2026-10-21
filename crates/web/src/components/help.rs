use yew::prelude::*;

use crate::l10n;

#[derive(Properties, PartialEq)]
pub(crate) struct Props {}

#[component(Help)]
pub(crate) fn help(_: &Props) -> Html {
    let l10n = l10n::use_phrases();
    let drag = l10n.text("help-key-drag");

    html! {
        <dl class="shortcuts">
            <dt><kbd>{"F1"}</kbd>{" / "}<kbd>{"?"}</kbd></dt>
            <dd>{l10n.text("help-show-help")}</dd>

            <dt><kbd>{"esc"}</kbd></dt>
            <dd>{l10n.text("help-close")}</dd>

            <dt><kbd>{"del"}</kbd></dt>
            <dd>{l10n.text("help-remove")}</dd>

            <dt><kbd>{l10n.text("help-key-lmb")}</kbd></dt>
            <dd>{l10n.text("help-select")}</dd>

            <dt><kbd>{"Shift"}</kbd>{" + "}<kbd>{drag.clone()}</kbd></dt>
            <dd>{l10n.text("help-facing")}</dd>

            <dt><kbd>{"Shift"}</kbd>{" + "}<kbd>{l10n.text("help-key-lmb")}</kbd></dt>
            <dd>{l10n.text("help-look-at")}</dd>

            <dt><kbd>{"S"}</kbd>{" + "}<kbd>{drag.clone()}</kbd></dt>
            <dd>{l10n.text("help-scale")}</dd>

            <dt><kbd>{"T"}</kbd></dt>
            <dd>{l10n.text("help-lock")}</dd>

            <dt><kbd>{l10n.text("help-key-mmb")}</kbd>{" + "}<kbd>{drag}</kbd></dt>
            <dd>{l10n.text("help-pan")}</dd>

            <dt><kbd>{l10n.text("help-key-scroll")}</kbd></dt>
            <dd>{l10n.text("help-zoom")}</dd>

            <dt><kbd>{"Enter"}</kbd></dt>
            <dd>{l10n.text("help-confirm")}</dd>
        </dl>
    }
}
