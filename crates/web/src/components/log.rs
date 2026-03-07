use yew::prelude::*;

use crate::log;

use super::Icon;

#[function_component(Log)]
pub(crate) fn log_component() -> Html {
    let log_context = use_context::<log::Log>().expect("ErrorLog context not found");
    let force_update = use_force_update();

    let _listener_handle = {
        let log_context = log_context.clone();
        let force_update = force_update.clone();

        use_memo((), move |_| {
            log_context.add_listener(Callback::from(move |_| {
                force_update.force_update();
            }))
        })
    };

    let entries = log_context.borrow_entries();

    let on_clear = {
        let log_context = log_context.clone();

        Callback::from(move |_| {
            log_context.clear();
        })
    };

    html! {
        <div id="content" class="log">
            <div class="log-header">
                <div class="log-actions">
                    <button class="btn" onclick={on_clear}>{"Clear"}</button>
                </div>
            </div>

            if entries.is_empty() {
                <p class="log-empty">{"Nothing has been logged"}</p>
            } else {
                <div class="log-entries">
                    {for entries.iter().rev().map(|entry| {
                        let severity_class = format!("log-entry severity-{}", entry.severity.as_str());

                        let icon = match entry.component.as_str() {
                            "remote-client" => Some(html!(<Icon name="remote" title="Remote Client" />)),
                            "mumble-link" => Some(html!(<Icon name="mumble" title="Mumble Link" />)),
                            _ => None,
                        };

                        html! {
                            <div class={severity_class}>
                                <div class="log-header">
                                    <span class="log-component">
                                        {icon}
                                        <span>{&entry.component}</span>
                                    </span>
                                    <span class="log-time">{entry.formatted_time()}</span>
                                </div>
                                <div class="log-message">{&entry.error}</div>
                            </div>
                        }
                    })}
                </div>
            }
        </div>
    }
}
