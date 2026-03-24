use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub(crate) struct Props {}

#[function_component(HelpModal)]
pub(crate) fn help_modal(_: &Props) -> Html {
    html! {
        <dl class="shortcuts">
            <dt><kbd>{"F1"}</kbd>{" / "}<kbd>{"?"}</kbd></dt>
            <dd>{"Show this help"}</dd>

            <dt><kbd>{"esc"}</kbd></dt>
            <dd>{"Close dialogs / Cancel"}</dd>

            <dt><kbd>{"del"}</kbd></dt>
            <dd>{"Remove selected"}</dd>

            <dt><kbd>{"LMB"}</kbd></dt>
            <dd>{"Select / Drag"}</dd>

            <dt><kbd>{"Shift"}</kbd>{" + "}<kbd>{"Drag"}</kbd></dt>
            <dd>{"Set facing"}</dd>

            <dt><kbd>{"Shift"}</kbd>{" + "}<kbd>{"LMB"}</kbd></dt>
            <dd>{"Set look at while object is selected"}</dd>

            <dt><kbd>{"S"}</kbd>{" + "}<kbd>{"Drag"}</kbd></dt>
            <dd>{"Scale selected object"}</dd>

            <dt><kbd>{"T"}</kbd></dt>
            <dd>{"Lock / unlock selected object"}</dd>

            <dt><kbd>{"MMB"}</kbd>{" + "}<kbd>{"Drag"}</kbd></dt>
            <dd>{"Pan map"}</dd>

            <dt><kbd>{"Scroll"}</kbd></dt>
            <dd>{"Zoom map"}</dd>

            <dt><kbd>{"Enter"}</kbd></dt>
            <dd>{"Confirm"}</dd>
        </dl>
    }
}
