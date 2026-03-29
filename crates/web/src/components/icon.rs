use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) name: String,
    #[prop_or_default]
    pub(crate) title: Option<String>,
    #[prop_or_default]
    pub(crate) invert: bool,
    #[prop_or_default]
    pub(crate) small: bool,
}

#[component(Icon)]
pub(crate) fn icon(props: &Props) -> Html {
    let title = props.title.clone();

    let class = match props.name.as_str() {
        "mumble" => "image-icon",
        _ => "icon",
    };

    let class = classes! {
        class,
        props.name.clone(),
        props.invert.then_some("invert"),
        props.small.then_some("sm"),
    };

    html! {
        <span {class} {title} />
    }
}
