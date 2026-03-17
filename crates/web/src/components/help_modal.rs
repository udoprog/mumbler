use yew::prelude::*;

use super::Icon;

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) onclose: Callback<()>,
}

#[function_component(HelpModal)]
pub(crate) fn help_modal(props: &Props) -> Html {
    html! {
        <div class="modal-backdrop" onclick={props.onclose.reform(|_| ())}>
            <div class="modal" onclick={|ev: MouseEvent| ev.stop_propagation()}>
                <div class="modal-header">
                    <h2>{"Keyboard Shortcuts"}</h2>
                    <button class="btn sm square danger" title="Close"
                        onclick={props.onclose.reform(|_| ())}>
                        <Icon name="x-mark" />
                    </button>
                </div>
                <div class="modal-body">
                    <dl class="shortcuts">
                        <dt><kbd>{"F1"}</kbd>{" / "}<kbd>{"?"}</kbd></dt>
                        <dd>{"Show this help"}</dd>

                        <dt><kbd>{"Escape"}</kbd></dt>
                        <dd>{"Close dialogs / Cancel"}</dd>

                        <dt><kbd>{"Delete"}</kbd></dt>
                        <dd>{"Delete selected"}</dd>

                        <dt><kbd>{"LMB"}</kbd></dt>
                        <dd>{"Select / Drag"}</dd>

                        <dt><kbd>{"Shift"}</kbd>{" + "}<kbd>{"Drag"}</kbd></dt>
                        <dd>{"Set facing"}</dd>

                        <dt><kbd>{"Shift"}</kbd>{" + "}<kbd>{"LMB"}</kbd></dt>
                        <dd>{"Set look at while object is selected"}</dd>

                        <dt><kbd>{"S"}</kbd>{" + "}<kbd>{"Drag"}</kbd></dt>
                        <dd>{"Scale selected object"}</dd>

                        <dt><kbd>{"MMB"}</kbd>{" + "}<kbd>{"Drag"}</kbd></dt>
                        <dd>{"Pan map"}</dd>

                        <dt><kbd>{"Scroll"}</kbd></dt>
                        <dd>{"Zoom map"}</dd>

                        <dt><kbd>{"Enter"}</kbd></dt>
                        <dd>{"Confirm"}</dd>
                    </dl>
                </div>
            </div>
        </div>
    }
}
