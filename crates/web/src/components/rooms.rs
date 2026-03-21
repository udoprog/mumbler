use core::cmp::Ordering;

use api::{Id, Key, LocalUpdateBody, PeerId, RemoteId, Type, Value};
use api::{RemoteObject, RemoteUpdateBody};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::error::Error;
use crate::log;

use super::Icon;

use super::into_target;

pub(crate) enum Msg {
    StateChanged(ws::State),
    Initialized(Result<Packet<api::InitializeRooms>, ws::Error>),
    LocalUpdate(Result<Packet<api::LocalUpdate>, ws::Error>),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    ConfigUpdate(Result<Packet<api::Update>, ws::Error>),
    Disconnect,
    Connect(RemoteId),
    ConnectResult(Result<Packet<api::Updates>, ws::Error>),
    CreateRoom,
    CreateRoomResult(Result<Packet<api::CreateObject>, ws::Error>),
    DeleteRoom(Id),
    DeleteRoomResult(Result<Packet<api::RemoveObject>, ws::Error>),
    NameChanged(Event),
    ContextUpdate(log::Log),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

struct Room {
    id: RemoteId,
    local: bool,
    name: String,
}

impl Room {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.name.cmp(&other.name) {
            Ordering::Equal => self.id.cmp(&other.id),
            other => other,
        }
    }

    fn from_remote(id: RemoteId, object: &RemoteObject, local: bool) -> Option<Self> {
        if object.ty != Type::ROOM {
            return None;
        }

        let name = object
            .props
            .get(Key::OBJECT_NAME)
            .as_str()
            .unwrap_or_default()
            .to_owned();

        Some(Self { id, local, name })
    }

    fn update(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::OBJECT_NAME => {
                self.name = value.as_str().unwrap_or_default().to_owned();
                true
            }
            _ => false,
        }
    }
}

pub(crate) struct Rooms {
    state: ws::State,
    rooms: Vec<Room>,
    peer_id: PeerId,
    active_room: RemoteId,
    new_room_name: String,
    log: log::Log,
    _log_handle: ContextHandle<log::Log>,
    _state_change: ws::StateListener,
    _init_request: ws::Request,
    _local_listener: ws::Listener,
    _remote_listener: ws::Listener,
    _config_listener: ws::Listener,
    _connect_room_request: ws::Request,
    _create_room_request: ws::Request,
    _delete_room_request: ws::Request,
}

impl Component for Rooms {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _log_handle) = ctx
            .link()
            .context::<log::Log>(ctx.link().callback(Msg::ContextUpdate))
            .expect("ErrorLog context not found");

        let (state, _state_change) = ctx
            .props()
            .ws
            .on_state_change(ctx.link().callback(Msg::StateChanged));

        let _local_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::LocalUpdate>(ctx.link().callback(Msg::LocalUpdate));

        let _remote_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::RemoteUpdate>(ctx.link().callback(Msg::RemoteUpdate));

        let _config_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::Update>(ctx.link().callback(Msg::ConfigUpdate));

        let mut this = Self {
            state,
            rooms: Vec::new(),
            peer_id: PeerId::ZERO,
            active_room: RemoteId::ZERO,
            new_room_name: String::new(),
            log,
            _log_handle,
            _state_change,
            _init_request: ws::Request::new(),
            _local_listener,
            _remote_listener,
            _config_listener,
            _connect_room_request: ws::Request::new(),
            _create_room_request: ws::Request::new(),
            _delete_room_request: ws::Request::new(),
        };

        this.refresh(ctx);
        this
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("rooms::update", error);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let on_create = ctx.link().callback(|_| Msg::CreateRoom);
        let on_name_changed = ctx.link().callback(Msg::NameChanged);

        html! {
            <div id="content" class="rows">
                if !self.rooms.is_empty() {
                    <section class="list">
                        <span class="list-title">{"Rooms"}</span>
                        {for self.rooms.iter().map(|room| self.view_room(ctx, room))}
                    </section>
                }

                <section class="input-group">
                    <input
                        type="text"
                        placeholder="New room"
                        value={self.new_room_name.clone()}
                        onchange={on_name_changed}
                    />

                    <button class="btn square" onclick={on_create}>
                        <Icon name="plus-circle" />
                    </button>
                </section>
            </div>
        }
    }
}

impl Rooms {
    fn view_room(&self, ctx: &Context<Self>, room: &Room) -> Html {
        let is_active = self.active_room == room.id;

        let delete_button = room.local.then(|| {
            let id = room.id.id;
            let onclick = ctx.link().callback(move |_| Msg::DeleteRoom(id));

            html! {
                <button class="btn square list-action" {onclick} title="Remove room">
                    <Icon name="trash" />
                </button>
            }
        });

        let on_connect = if is_active {
            ctx.link().callback(move |_| Msg::Disconnect)
        } else {
            let room = room.id;
            ctx.link().callback(move |_| Msg::Connect(room))
        };

        let icon = if is_active { "link-slash" } else { "link" };
        let class = classes! {
            "btn",
            "square",
            "list-action",
            is_active.then_some("active"),
            is_active.then_some("primary"),
        };

        let connect_button = html! {
            <button {class} onclick={on_connect}>
                <Icon name={icon} />
            </button>
        };

        let room_icon = if room.local { "home" } else { "home-modern" };

        html! {
            <div class="list-content" key={room.id}>
                <Icon name={room_icon} invert={true} />
                <span class="list-label" title={room.id.to_string()}>{&room.name}</span>
                {connect_button}
                {delete_button}
            </div>
        }
    }

    fn refresh(&mut self, ctx: &Context<Self>) {
        if matches!(self.state, ws::State::Open) {
            self._init_request = ctx
                .props()
                .ws
                .request()
                .body(api::InitializeRoomsRequest)
                .on_packet(ctx.link().callback(Msg::Initialized))
                .send();
        } else {
            self.peer_id = PeerId::ZERO;
            self.active_room = RemoteId::ZERO;
            self.rooms.clear();
        }
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::StateChanged(state) => {
                self.state = state;
                self.rooms.clear();
                self.refresh(ctx);
                Ok(true)
            }
            Msg::Initialized(body) => {
                let body = body?;
                let body = body.decode()?;

                self.peer_id = *body.config.get(Key::PEER_ID).as_peer_id();
                self.active_room = *body.config.get(Key::ROOM).as_remote_id();
                self.rooms.clear();

                for object in body.local {
                    let id = RemoteId::new(self.peer_id, object.id);

                    if let Some(room) = Room::from_remote(id, &object, true) {
                        self.rooms.push(room);
                    }
                }

                for peer in body.peers {
                    for object in peer.objects {
                        let id = RemoteId::new(peer.peer_id, object.id);

                        if let Some(room) = Room::from_remote(id, &object, false) {
                            self.rooms.push(room);
                        }
                    }
                }

                self.rooms.sort_by(Room::cmp);
                Ok(true)
            }
            Msg::LocalUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                match body {
                    LocalUpdateBody::ObjectCreated { object } => {
                        let id = RemoteId::new(self.peer_id, object.id);

                        if let Some(room) = Room::from_remote(id, &object, true) {
                            self.rooms.push(room);
                            self.rooms.sort_by(Room::cmp);
                            return Ok(true);
                        }

                        Ok(true)
                    }
                    LocalUpdateBody::ObjectRemoved { id: object_id } => {
                        let prev = self.rooms.len();
                        self.rooms.retain(|r| r.local && r.id.id != object_id);
                        Ok(self.rooms.len() != prev)
                    }
                    LocalUpdateBody::ObjectUpdated {
                        id: object_id,
                        key,
                        value,
                    } => {
                        let Some(room) = self
                            .rooms
                            .iter_mut()
                            .find(|r| r.local && r.id.id == object_id)
                        else {
                            return Ok(false);
                        };

                        let update = room.update(key, value);

                        if update {
                            self.rooms.sort_by(Room::cmp);
                        }

                        Ok(update)
                    }
                    _ => Ok(false),
                }
            }
            Msg::RemoteUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                match body {
                    RemoteUpdateBody::RemoteLost => {
                        self.rooms.retain(|r| r.local);
                        Ok(true)
                    }
                    RemoteUpdateBody::PeerConnected {
                        peer_id, objects, ..
                    } => {
                        for object in objects {
                            let id = RemoteId::new(peer_id, object.id);

                            if let Some(room) = Room::from_remote(id, &object, false) {
                                self.rooms.push(room);
                                self.rooms.sort_by(Room::cmp);
                            }
                        }

                        Ok(true)
                    }
                    RemoteUpdateBody::PeerDisconnect { peer_id } => {
                        self.rooms.retain(|r| r.id.peer_id != peer_id);
                        Ok(true)
                    }
                    RemoteUpdateBody::ObjectCreated { id, object } => {
                        if let Some(room) = Room::from_remote(id, &object, false) {
                            self.rooms.push(room);
                            self.rooms.sort_by(Room::cmp);
                            Ok(true)
                        } else {
                            Ok(false)
                        }
                    }
                    RemoteUpdateBody::ObjectRemoved { id } => {
                        let prev = self.rooms.len();
                        self.rooms.retain(|r| r.id != id);
                        Ok(self.rooms.len() != prev)
                    }
                    RemoteUpdateBody::ObjectUpdated { id, key, value } => {
                        let Some(entry) = self.rooms.iter_mut().find(|r| r.id == id) else {
                            return Ok(false);
                        };

                        let update = entry.update(key, value);

                        if update {
                            self.rooms.sort_by(Room::cmp);
                        }

                        Ok(update)
                    }
                    _ => Ok(false),
                }
            }
            Msg::ConfigUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                match body.key {
                    Key::ROOM => {
                        self.active_room = *body.value.as_remote_id();
                        Ok(true)
                    }
                    _ => Ok(false),
                }
            }
            Msg::Disconnect => {
                let values = vec![(Key::ROOM, Value::empty())];

                self._connect_room_request = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UpdatesRequest { values })
                    .on_packet(ctx.link().callback(Msg::ConnectResult))
                    .send();

                Ok(false)
            }
            Msg::Connect(room) => {
                let values = vec![(Key::ROOM, Value::from(room))];

                self._connect_room_request = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UpdatesRequest { values })
                    .on_packet(ctx.link().callback(Msg::ConnectResult))
                    .send();

                Ok(false)
            }
            Msg::ConnectResult(body) => {
                let body = body?;
                _ = body.decode()?;
                Ok(false)
            }
            Msg::CreateRoom => {
                let name = self.new_room_name.trim().to_owned();

                if name.is_empty() {
                    return Ok(false);
                }

                let props = api::Properties::from([(Key::OBJECT_NAME, Value::from(name))]);

                self._create_room_request = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::CreateObjectRequest {
                        ty: Type::ROOM,
                        props,
                    })
                    .on_packet(ctx.link().callback(Msg::CreateRoomResult))
                    .send();

                Ok(false)
            }
            Msg::CreateRoomResult(body) => {
                let body = body?;
                _ = body.decode()?;
                self.new_room_name = String::new();
                Ok(true)
            }
            Msg::DeleteRoom(id) => {
                self._delete_room_request = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::RemoveObjectRequest { id })
                    .on_packet(ctx.link().callback(Msg::DeleteRoomResult))
                    .send();

                Ok(false)
            }
            Msg::DeleteRoomResult(body) => {
                let body = body?;
                _ = body.decode()?;
                Ok(false)
            }
            Msg::NameChanged(e) => {
                e.stop_propagation();
                let input = into_target!(e, HtmlInputElement);
                self.new_room_name = input.value();
                Ok(true)
            }
            Msg::ContextUpdate(log) => {
                self.log = log;
                Ok(false)
            }
        }
    }
}
