use yew::prelude::*;
use yew_router::prelude::*;

use crate::log::{self, Severity};

#[derive(Debug, Clone, Copy, PartialEq, Routable)]
pub enum Route {
    #[at("/")]
    Map,
    #[at("/settings")]
    Settings,
    #[at("/log")]
    Log,
    #[not_found]
    #[at("/404")]
    NotFound,
}

#[derive(Properties, PartialEq)]
pub struct NavigationProps {
    pub route: Route,
}

#[function_component(Navigation)]
pub(crate) fn navigation(props: &NavigationProps) -> Html {
    let log_context = use_context::<log::Log>().expect("ErrorLog context not found");
    let error_count = use_state(|| {
        log_context
            .borrow_entries()
            .iter()
            .filter(|e| e.severity == Severity::Error)
            .count()
    });
    let info_count = use_state(|| {
        log_context
            .borrow_entries()
            .iter()
            .filter(|e| e.severity == Severity::Info)
            .count()
    });

    {
        let log_context = log_context.clone();
        let error_count = error_count.clone();
        let info_count = info_count.clone();

        use_memo((), move |_| {
            let log_context_inner = log_context.clone();

            log_context.add_listener(Callback::from(move |_| {
                let entries = log_context_inner.borrow_entries();
                error_count.set(
                    entries
                        .iter()
                        .filter(|e| e.severity == Severity::Error)
                        .count(),
                );
                info_count.set(
                    entries
                        .iter()
                        .filter(|e| e.severity == Severity::Info)
                        .count(),
                );
            }))
        });
    }

    html! {
        <section class="navigation">
            <Link<Route> to={Route::Map} classes={classes!((props.route == Route::Map).then_some("active"))}>
                {"Map"}
            </Link<Route>>
            <Link<Route> to={Route::Settings} classes={classes!((props.route == Route::Settings).then_some("active"))}>
                {"Settings"}
            </Link<Route>>
            <Link<Route> to={Route::Log} classes={classes!((props.route == Route::Log).then_some("active"))}>
                {"Log"}
                if *error_count > 0 {
                    <span class="badge error">{*error_count}</span>
                }
                if *info_count > 0 {
                    <span class="badge info">{*info_count}</span>
                }
            </Link<Route>>
        </section>
    }
}
