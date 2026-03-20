use api::Id;
use yew::prelude::*;
use yew::virtual_dom::{VList, VNode};

use crate::drag_over::DragOver;
use crate::hierarchy::Hierarchy;
use crate::objects::{ObjectKind, Objects};

use super::Icon;

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) group: Id,
    pub(crate) drag_over: Option<DragOver>,
    pub(crate) mumble_object: Option<Id>,
    #[prop_or_default]
    pub(crate) drop_into_last: Option<Id>,
    pub(crate) selected: Option<Id>,
    pub(crate) onselect: Callback<Id>,
    pub(crate) ondragend: Callback<Id>,
    pub(crate) ondragover: Callback<DragOver>,
    pub(crate) onhiddentoggle: Callback<Id>,
    pub(crate) onlocalhiddentoggle: Callback<Id>,
    pub(crate) onexpandtoggle: Callback<Id>,
    pub(crate) onlockedtoggle: Callback<Id>,
    pub(crate) onmumbletoggle: Callback<Id>,
}

pub(crate) enum Msg {
    SetHierarchy(Hierarchy),
    SetObjects(Objects),
}

pub(crate) struct ObjectList {
    order: Hierarchy,
    _order_handle: ContextHandle<Hierarchy>,
    objects: Objects,
    _objects_handle: ContextHandle<Objects>,
}

impl Component for ObjectList {
    type Properties = Props;
    type Message = Msg;

    fn create(ctx: &Context<Self>) -> Self {
        let (order, _order_handle) = ctx
            .link()
            .context::<Hierarchy>(ctx.link().callback(Msg::SetHierarchy))
            .expect("hirearchy context not found");

        let (objects, _objects_handle) = ctx
            .link()
            .context::<Objects>(ctx.link().callback(Msg::SetObjects))
            .expect("hirearchy context not found");

        Self {
            order,
            _order_handle,
            objects,
            _objects_handle,
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Msg) -> bool {
        match msg {
            Msg::SetHierarchy(order) => {
                self.order = order;
                true
            }
            Msg::SetObjects(objects) => {
                self.objects = objects;
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let mut list = Vec::new();

        let order = self.order.borrow();
        let objects = self.objects.borrow();

        let group = ctx.props().group;

        let mut it = order.iter(group).flat_map(|id| objects.get(id));
        let last = it.next_back().map(|o| (true, o));
        let remaining = it.map(|o| (false, o));

        let mut is_empty = true;

        for (n, (is_last, o)) in remaining.chain(last).enumerate() {
            is_empty = false;

            let target = o.id;
            let selected = ctx.props().selected == Some(target);

            let label = o.name().unwrap_or("");

            let onclick = ctx.props().onselect.reform(move |ev: MouseEvent| {
                ev.stop_propagation();
                target
            });

            let ondragend = ctx.props().ondragend.reform(move |ev: DragEvent| {
                ev.stop_propagation();
                target
            });

            let ondragstart = ctx.props().ondragover.reform(move |ev: DragEvent| {
                ev.stop_propagation();
                DragOver::below(group, target)
            });

            let drag = if o.is_group() {
                DragOver::into
            } else {
                DragOver::below
            };

            let ondragover = ctx.props().ondragover.reform(move |ev: DragEvent| {
                ev.stop_propagation();
                drag(group, target)
            });

            if n == 0 {
                let class = classes! {
                    "object-drop",
                    (ctx.props().drag_over == Some(DragOver::above(group, target))).then_some("active"),
                };

                let ondragover = ctx.props().ondragover.reform(move |ev: DragEvent| {
                    ev.stop_propagation();
                    DragOver::above(group, target)
                });

                list.push(html! {
                    <div key={format!("drop-above-{target}")} {class} {ondragover} />
                });
            }

            let mumble_button = o.is_interactive().then(|| {
                let is_mumble = ctx.props().mumble_object == Some(target);

                let class = classes! {
                    "btn", "sm", "square", "object-action",
                    is_mumble.then_some("success"),
                    is_mumble.then_some("active"),
                };

                let onclick = ctx.props().onmumbletoggle.reform(move |ev: MouseEvent| {
                    ev.stop_propagation();
                    target
                });

                html! {
                    <button {class} title="Toggle as MumbleLink Source" {onclick}>
                    <Icon name="mumble" />
                    </button>
                }
            });

            let expand_button = o.is_group().then(|| {
                let is_expanded = o.is_expanded();

                let class = classes! {
                    "btn", "sm", "square", "object-action",
                    is_expanded.then_some("success"),
                    is_expanded.then_some("active"),
                };

                let onclick = ctx.props().onexpandtoggle.reform(move |ev: MouseEvent| {
                    ev.stop_propagation();
                    target
                });

                html! {
                    <button {class} title="Expand or collapse group" {onclick}>
                        <Icon name="folder-open" />
                    </button>
                }
            });

            let hidden_button = {
                let is_hidden = o.is_hidden();

                let class = classes! {
                    "btn", "sm", "square", "object-action",
                    is_hidden.then_some("danger"),
                    is_hidden.then_some("active"),
                };

                let title = if is_hidden {
                    "Hidden from others"
                } else {
                    "Visible to others"
                };

                let onclick = ctx.props().onhiddentoggle.reform(move |ev: MouseEvent| {
                    ev.stop_propagation();
                    target
                });

                let name = if is_hidden { "link-slash" } else { "link" };

                html! {
                    <button {class} {title} {onclick}>
                        <Icon {name} />
                    </button>
                }
            };

            let local_hidden_button = {
                let is_local_hidden = o.is_local_hidden();

                let class = classes! {
                    "btn", "sm", "square", "object-action",
                    is_local_hidden.then_some("danger"),
                    is_local_hidden.then_some("active"),
                };

                let title = if is_local_hidden {
                    "Hidden locally"
                } else {
                    "Visible locally"
                };

                let onclick = ctx
                    .props()
                    .onlocalhiddentoggle
                    .reform(move |ev: MouseEvent| {
                        ev.stop_propagation();
                        target
                    });

                let name = if is_local_hidden { "eye-slash" } else { "eye" };

                html! {
                    <button {class} {title} {onclick}>
                        <Icon {name} />
                    </button>
                }
            };

            let locked_button = {
                let is_locked = o.is_locked();

                let class = classes! {
                    "btn", "sm", "square", "object-action",
                    is_locked.then_some("danger"),
                    is_locked.then_some("active"),
                };

                let title = if is_locked { "Locked" } else { "Unlocked" };

                let onclick = ctx.props().onlockedtoggle.reform(move |ev: MouseEvent| {
                    ev.stop_propagation();
                    target
                });

                let name = if is_locked {
                    "lock-closed"
                } else {
                    "lock-open"
                };

                html! {
                    <button {class} {title} {onclick}>
                        <Icon {name} />
                    </button>
                }
            };

            let drop_into_last =
                (ctx.props().drag_over == Some(DragOver::into(group, o.id))).then_some(group);

            let class = classes! {
                "object-content",
                selected.then_some("selected"),
            };

            list.push(html! {
                <section
                    key={format!("drag-{target}")}
                    class="object-drag"
                    draggable={true}
                    {onclick}
                    {ondragstart}
                    {ondragend}
                    {ondragover}
                >
                    <section {class}>
                        <Icon name={o.icon()} invert={true} small={true} />

                        <span class="object-label">{label}</span>

                        {mumble_button}

                        {expand_button}

                        {hidden_button}

                        {local_hidden_button}

                        {locked_button}
                    </section>
                </section>
            });

            list.extend(match &o.kind {
                ObjectKind::Group(g) => (g.is_expanded()
                    && (drop_into_last.is_some() || !order.is_empty(target)))
                .then(|| {
                    html! {
                        <section key={format!("{target}-children")} class="object-children">
                            <ObjectList
                                key={format!("{}", o.id)}
                                group={o.id}
                                drag_over={ctx.props().drag_over}
                                mumble_object={ctx.props().mumble_object}
                                {drop_into_last}
                                selected={ctx.props().selected}
                                onselect={ctx.props().onselect.clone()}
                                ondragover={ctx.props().ondragover.clone()}
                                ondragend={ctx.props().ondragend.clone()}
                                onhiddentoggle={ctx.props().onhiddentoggle.clone()}
                                onlocalhiddentoggle={ctx.props().onlocalhiddentoggle.clone()}
                                onexpandtoggle={ctx.props().onexpandtoggle.clone()}
                                onlockedtoggle={ctx.props().onlockedtoggle.clone()}
                                onmumbletoggle={ctx.props().onmumbletoggle.clone()}
                                />

                        </section>
                    }
                }),
                _ => None,
            });

            let class = classes! {
                "object-drop",
                (is_last && ctx.props().drop_into_last.is_some() || ctx.props().drag_over == Some(DragOver::below(group, target))).then_some("active"),
            };

            let ondragover = ctx.props().ondragover.reform(move |ev: DragEvent| {
                ev.stop_propagation();
                DragOver::below(group, target)
            });

            list.push(html! {
                <div key={format!("drag-below-{target}")} {class} {ondragover} />
            });
        }

        if is_empty && let Some(target) = ctx.props().drop_into_last {
            let ondragover = ctx.props().ondragover.reform(move |ev: DragEvent| {
                ev.stop_propagation();
                DragOver::into(group, target)
            });

            list.push(html! {
                <div key={format!("drag-last")} class="object-drop active" {ondragover} />
            });
        }

        let class = classes! {
            "object-list",
            ctx.props().drag_over.is_some().then_some("dragging"),
        };
        let objects = VNode::from(VList::from(list));

        html! {
            <div key={"objects-list"} {class}>{objects}</div>
        }
    }
}
