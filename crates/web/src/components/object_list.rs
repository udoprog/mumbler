use api::Id;
use yew::prelude::*;
use yew::virtual_dom::{VList, VNode};

use crate::hierarchy::Hierarchy;
use crate::objects::{ObjectKind, Objects};

use super::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Drag {
    Above,
    Into,
    Below,
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) group: Id,
    pub(crate) drag_over: Option<(Drag, Id, Id)>,
    pub(crate) mumble_object: Option<Id>,
    pub(crate) selected: Option<Id>,
    pub(crate) onselect: Callback<Id>,
    pub(crate) ondragend: Callback<Id>,
    pub(crate) ondragover: Callback<(Drag, Id, Id)>,
    pub(crate) onhiddentoggle: Callback<Id>,
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

        for (n, o) in order.iter(group).flat_map(|id| objects.get(id)).enumerate() {
            let (icon_name, mumble_button, is_group) = match o.kind {
                ObjectKind::Token(..) => ("user", true, false),
                ObjectKind::Static(..) => ("squares-2x2", true, false),
                ObjectKind::Group(..) => ("folder", false, true),
                _ => ("question-mark-circle", false, false),
            };

            let id = o.id;
            let selected = ctx.props().selected == Some(id);

            let label = o.name().unwrap_or("");

            let onclick = ctx.props().onselect.reform(move |ev: MouseEvent| {
                ev.stop_propagation();
                id
            });

            let ondragend = ctx.props().ondragend.reform(move |ev: DragEvent| {
                ev.stop_propagation();
                id
            });

            let ondragstart = ctx.props().ondragover.reform(move |ev: DragEvent| {
                ev.stop_propagation();
                (Drag::Below, group, id)
            });

            let drag_into = if is_group { Drag::Into } else { Drag::Below };

            let ondragover = ctx.props().ondragover.reform(move |ev: DragEvent| {
                ev.stop_propagation();
                (drag_into, group, id)
            });

            if n == 0 {
                let class = classes! {
                    "object-drop",
                    (ctx.props().drag_over == Some((Drag::Above, group, id))).then_some("active"),
                };

                let ondragover = ctx.props().ondragover.reform(move |ev: DragEvent| {
                    ev.stop_propagation();
                    (Drag::Above, group, id)
                });

                list.push(html! {
                    <div key={format!("drop-above-{id}")} {class} {ondragover} />
                });
            }

            let is_hidden = o.is_hidden();
            let is_locked = o.is_locked();
            let hidden_icon = if is_hidden { "eye-slash" } else { "eye" };
            let hidden_title = if is_hidden {
                "Hidden from others"
            } else {
                "Visible to others"
            };

            let hidden_onclick = ctx.props().onhiddentoggle.reform(move |ev: MouseEvent| {
                ev.stop_propagation();
                id
            });

            let locked_icon = if is_locked {
                "lock-closed"
            } else {
                "lock-open"
            };
            let locked_title = if is_locked { "Locked" } else { "Unlocked" };

            let locked_onclick = ctx.props().onlockedtoggle.reform(move |ev: MouseEvent| {
                ev.stop_propagation();
                id
            });

            let is_mumble = ctx.props().mumble_object == Some(id);

            let mumble_classes = classes! {
                "btn", "sm", "square", "object-action",
                is_mumble.then_some("success"),
                is_mumble.then_some("active"),
                (!mumble_button).then_some("disabled"),
            };

            let mumble_onclick = mumble_button.then(|| {
                ctx.props().onmumbletoggle.reform(move |ev: MouseEvent| {
                    ev.stop_propagation();
                    id
                })
            });

            let hidden_classes = classes! {
                "btn", "sm", "square", "object-action",
                is_hidden.then_some("danger"),
                is_hidden.then_some("active"),
            };

            let locked_classes = classes! {
                "btn", "sm", "square", "object-action",
                is_locked.then_some("danger"),
                is_locked.then_some("active"),
            };

            let drop_into = (ctx.props().drag_over == Some((Drag::Into, group, o.id))).then(|| {
                html! {
                    <div key={format!("drop-into")} class="object-drop active" />
                }
            });

            let children = match o.kind {
                ObjectKind::Group(..) => Some(html! {
                    <section key={format!("{id}-children")} class="object-children">
                        <ObjectList
                            key={format!("{}", o.id)}
                            group={o.id}
                            drag_over={ctx.props().drag_over}
                            mumble_object={ctx.props().mumble_object}
                            selected={ctx.props().selected}
                            onselect={ctx.props().onselect.clone()}
                            ondragover={ctx.props().ondragover.clone()}
                            ondragend={ctx.props().ondragend.clone()}
                            onhiddentoggle={ctx.props().onhiddentoggle.clone()}
                            onlockedtoggle={ctx.props().onlockedtoggle.clone()}
                            onmumbletoggle={ctx.props().onmumbletoggle.clone()}
                            />
                        {drop_into}
                    </section>
                }),
                _ => None,
            };

            let class = classes! {
                "object-button",
                selected.then_some("selected"),
            };

            let sort = format!("{:?}", o.sort());

            let node = html! {
                <div key={format!("object-{id}")} class="object-item">
                    <section
                        key={format!("drag-{id}")}
                        class="object-drag"
                        draggable={true}
                        {onclick}
                        {ondragstart}
                        {ondragend}
                        {ondragover}
                    >
                        <section {class} title={sort}>
                            <Icon name={icon_name} invert={true} small={true} />

                            <span class="object-label">{label}</span>

                            <button class={mumble_classes}
                                title="Toggle as MumbleLink Source"
                                onclick={mumble_onclick}>
                                <Icon name="mumble" />
                            </button>

                            <button class={hidden_classes}
                                title={hidden_title}
                                onclick={hidden_onclick}>
                                <Icon name={hidden_icon} />
                            </button>

                            <button class={locked_classes}
                                title={locked_title}
                                onclick={locked_onclick}>
                                <Icon name={locked_icon} />
                            </button>
                        </section>
                    </section>

                    {children}
                </div>
            };

            list.push(node);

            let class = classes! {
                "object-drop",
                (ctx.props().drag_over == Some((Drag::Below, group, id))).then_some("active"),
            };

            let ondragover = ctx.props().ondragover.reform(move |ev: DragEvent| {
                ev.stop_propagation();
                (Drag::Below, group, id)
            });

            list.push(html! {
                <div key={format!("drag-below-{id}")} {class} {ondragover} />
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
