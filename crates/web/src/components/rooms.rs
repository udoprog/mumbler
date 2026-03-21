use core::cmp::Ordering;
use std::collections::HashMap;

use api::{
    Id, Key, LocalUpdateBody, PeerId, PublicKey, RemoteId, StableId, Type, UpdateBody, Value,
};
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
    Connect(StableId),
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
    id: StableId,
    name: String,
}

impl Room {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.name.cmp(&other.name) {
            Ordering::Equal => self.id.cmp(&other.id),
            other => other,
        }
    }

    fn from_remote(id: StableId, object: &RemoteObject) -> Option<Self> {
        if object.ty != Type::ROOM {
            return None;
        }

        let name = object
            .props
            .get(Key::OBJECT_NAME)
            .as_str()
            .unwrap_or_default()
            .to_owned();

        Some(Self { id, name })
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
    peers: HashMap<PeerId, PublicKey>,
    rooms: Vec<Room>,
    public_key: PublicKey,
    active_room: StableId,
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
            peers: HashMap::new(),
            rooms: Vec::new(),
            public_key: PublicKey::ZERO,
            active_room: StableId::ZERO,
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
                    <section class="list" key="rooms-list">
                        <span class="list-title">{"Rooms"}</span>
                        {for self.rooms.iter().map(|room| self.view_room(ctx, room))}
                    </section>
                }

                <section class="input-group" key="rooms-create">
                    <input
                        type="text"
                        placeholder="New room"
                        value={self.new_room_name.clone()}
                        onchange={on_name_changed}
                    />

                    <button class="btn lg square" onclick={on_create}>
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
        let is_local = room.id.public_key == self.public_key;

        let delete_button = is_local.then(|| {
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

        let room_icon = if is_local { "home" } else { "home-modern" };

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
            self.public_key = PublicKey::ZERO;
            self.active_room = StableId::ZERO;
            self.rooms.clear();
        }
    }

    fn to_stable_id(&self, id: RemoteId) -> StableId {
        let Some(public_key) = self.peers.get(&id.peer_id) else {
            return StableId::ZERO;
        };

        StableId::new(*public_key, id.id)
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

                self.public_key = body.public_key;
                self.active_room = *body.props.get(Key::ROOM).as_stable_id();
                self.rooms.clear();

                for object in body.local {
                    let id = StableId::new(self.public_key, object.id);

                    if let Some(room) = Room::from_remote(id, &object) {
                        self.rooms.push(room);
                    }
                }

                for peer in body.peers {
                    self.peers.insert(peer.peer_id, peer.public_key);

                    for object in peer.objects {
                        let id = StableId::new(peer.public_key, object.id);

                        if let Some(room) = Room::from_remote(id, &object) {
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
                        let id = StableId::new(self.public_key, object.id);

                        if let Some(room) = Room::from_remote(id, &object) {
                            self.rooms.push(room);
                            self.rooms.sort_by(Room::cmp);
                            return Ok(true);
                        }

                        Ok(true)
                    }
                    LocalUpdateBody::ObjectRemoved { id } => {
                        let id = StableId::new(self.public_key, id);

                        let prev = self.rooms.len();
                        self.rooms.retain(|r| r.id != id);
                        Ok(self.rooms.len() != prev)
                    }
                    LocalUpdateBody::ObjectUpdated { id, key, value } => {
                        let id = StableId::new(self.public_key, id);

                        let Some(room) = self.rooms.iter_mut().find(|r| r.id == id) else {
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
                        self.rooms.retain(|r| r.id.public_key != self.public_key);
                        Ok(true)
                    }
                    RemoteUpdateBody::PeerConnected {
                        peer_id,
                        public_key,
                        objects,
                        ..
                    } => {
                        self.peers.insert(peer_id, public_key);

                        for object in objects {
                            let id = StableId::new(public_key, object.id);

                            if let Some(room) = Room::from_remote(id, &object) {
                                self.rooms.push(room);
                                self.rooms.sort_by(Room::cmp);
                            }
                        }

                        Ok(true)
                    }
                    RemoteUpdateBody::PeerDisconnect { peer_id } => {
                        let Some(public_key) = self.peers.remove(&peer_id) else {
                            return Ok(false);
                        };

                        self.rooms.retain(|r| r.id.public_key != public_key);
                        Ok(true)
                    }
                    RemoteUpdateBody::ObjectCreated { id, object } => {
                        let id = self.to_stable_id(id);

                        if let Some(room) = Room::from_remote(id, &object) {
                            self.rooms.push(room);
                            self.rooms.sort_by(Room::cmp);
                            Ok(true)
                        } else {
                            Ok(false)
                        }
                    }
                    RemoteUpdateBody::ObjectRemoved { id } => {
                        let id = self.to_stable_id(id);

                        let prev = self.rooms.len();
                        self.rooms.retain(|r| r.id != id);
                        Ok(self.rooms.len() != prev)
                    }
                    RemoteUpdateBody::ObjectUpdated { id, key, value } => {
                        let id = self.to_stable_id(id);

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

                match body {
                    UpdateBody::Config { key, value } => match key {
                        Key::ROOM => {
                            self.active_room = *value.as_stable_id();
                            Ok(true)
                        }
                        _ => Ok(false),
                    },
                    UpdateBody::PublicKey { public_key } => {
                        self.public_key = public_key;
                        Ok(true)
                    }
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
