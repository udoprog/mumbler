use api::{Color, Id, Key, Value};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::error::Error;
use crate::log;
use crate::state::State;

use super::into_target;

pub(crate) enum Msg {
    ColorChanged(Event),
    GetObjectSettings(Result<Packet<api::GetObjectSettings>, ws::Error>),
    LocalUpdate(Result<Packet<api::LocalUpdate>, ws::Error>),
    NameChanged(Event),
    SelectColor(api::Color),
    SetLog(log::Log),
    StateChanged(ws::State),
    UpdateName(Option<String>),
    UpdateResult(Result<Packet<api::Update>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
    pub(crate) id: Id,
}

pub(crate) struct GroupSettings {
    _list_settings: ws::Request,
    _local_update_listener: ws::Listener,
    _log_handle: ContextHandle<log::Log>,
    _select_color: ws::Request,
    _state_change: ws::StateListener,
    _update_name: ws::Request,
    color: State<Option<api::Color>>,
    log: log::Log,
    name: State<Option<String>>,
    state: ws::State,
}

impl Component for GroupSettings {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _log_handle) = ctx
            .link()
            .context::<log::Log>(ctx.link().callback(Msg::SetLog))
            .expect("ErrorLog context not found");

        let (state, _state_change) = ctx
            .props()
            .ws
            .on_state_change(ctx.link().callback(Msg::StateChanged));

        let _local_update_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::LocalUpdate>(ctx.link().callback(Msg::LocalUpdate));

        let mut this = Self {
            state,
            color: State::new(None),
            name: State::new(None),
            log,
            _log_handle,
            _state_change,
            _list_settings: ws::Request::new(),
            _select_color: ws::Request::new(),
            _update_name: ws::Request::new(),
            _local_update_listener,
        };

        this.refresh(ctx);
        this
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("static_settings::update", error);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let color = self.color.unwrap_or_else(Color::neutral);

        html! {
            <>
            <div id="content" class="row">
                <div class="rows">
                    <section class="input-group">
                        <label for="group-name">{"Name:"}</label>

                        <input
                            id="group-name"
                            type="text"
                            placeholder="Enter name"
                            value={(*self.name).clone().unwrap_or_default()}
                            onchange={ctx.link().callback(Msg::NameChanged)}
                            />
                    </section>

                    <section class="input-group">
                        <label for="group-color">
                            {"Color:"}
                            <span class="color-preview" style={format!("--color: {}", color.to_css_string())} />
                        </label>

                        <input
                            id="group-color"
                            class="hidden"
                            type="color"
                            value={color.to_css_string()}
                            onchange={ctx.link().callback(Msg::ColorChanged)}
                            />
                    </section>
                </div>
            </div>
            </>
        }
    }
}

impl GroupSettings {
    fn refresh(&mut self, ctx: &Context<Self>) {
        if matches!(self.state, ws::State::Open) {
            self._list_settings = ctx
                .props()
                .ws
                .request()
                .body(api::GetObjectSettingsRequest { id: ctx.props().id })
                .on_packet(ctx.link().callback(Msg::GetObjectSettings))
                .send();
        }
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::StateChanged(state) => {
                self.state = state;
                self.refresh(ctx);
                Ok(true)
            }
            Msg::GetObjectSettings(result) => {
                let body = result?;
                let body = body.decode()?;

                for (key, value) in body.object.props {
                    self.update_property(ctx, key, value);
                }

                Ok(true)
            }
            Msg::ColorChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let hex_string = input.value();

                if let Some(color) = api::Color::from_hex(&hex_string) {
                    ctx.link().send_message(Msg::SelectColor(color));
                }

                Ok(false)
            }
            Msg::SelectColor(color) => {
                *self.color = Some(color);
                self._select_color = send_update(ctx, Key::COLOR, color);
                Ok(true)
            }
            Msg::NameChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let value = input.value();
                let name = if value.is_empty() { None } else { Some(value) };
                ctx.link().send_message(Msg::UpdateName(name));
                Ok(false)
            }
            Msg::UpdateName(name) => {
                *self.name = name.clone();
                self._update_name = send_update(ctx, Key::NAME, name);
                Ok(true)
            }
            Msg::SetLog(log) => {
                self.log = log;
                Ok(false)
            }
            Msg::UpdateResult(result) => {
                let result = result?;
                _ = result.decode()?;
                Ok(false)
            }
            Msg::LocalUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                let changed = match body {
                    api::LocalUpdateBody::Update {
                        object_id,
                        key,
                        value,
                    } => {
                        if object_id != ctx.props().id {
                            return Ok(false);
                        }

                        self.update_property(ctx, key, value)
                    }
                    _ => return Ok(false),
                };

                Ok(changed)
            }
        }
    }

    fn update_property(&mut self, _: &Context<Self>, key: Key, value: Value) -> bool {
        match key {
            Key::COLOR => self.color.update(value.as_color()),
            Key::NAME => self.name.update(value.as_str().map(str::to_owned)),
            _ => false,
        }
    }
}

fn send_update(ctx: &Context<GroupSettings>, key: Key, value: impl Into<Value>) -> ws::Request {
    ctx.props()
        .ws
        .request()
        .body(api::UpdateRequest {
            object_id: ctx.props().id,
            key,
            value: value.into(),
        })
        .on_packet(ctx.link().callback(Msg::UpdateResult))
        .send()
}
