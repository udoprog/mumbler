use core::cmp::Ordering;
use std::collections::HashMap;

use api::{Id, Key, PeerId, PublicKey, RemoteId, StableId, Type, UpdateBody, Value};
use api::{RemoteObject, RemoteUpdateBody};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use yew::prelude::*;

use crate::error::Error;
use crate::log;
use crate::state::State;

use super::{COMMON_ROOM_NAME, Icon};

pub(crate) enum Msg {
    StateChanged(ws::State),
    Initialized(Result<Packet<api::InitializeRooms>, ws::Error>),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    ConfigUpdate(Result<Packet<api::Update>, ws::Error>),
    Disconnect,
    Connect(StableId),
    ConnectResult(Result<Packet<api::Updates>, ws::Error>),
    CreateRoom,
    CreateRoomResult(Result<Packet<api::CreateObject>, ws::Error>),
    ContextUpdate(log::Log),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
    pub(crate) onopensettings: Callback<Id>,
    pub(crate) onrequestdelete: Callback<(Id, String)>,
}

struct Room {
    id: StableId,
    name: String,
}

struct Peer {
    public_key: PublicKey,
    room: State<StableId>,
    name: State<String>,
}

impl Peer {
    fn new(public_key: PublicKey, props: &api::Properties) -> Self {
        Self {
            public_key,
            room: State::new(*props.get(Key::ROOM).as_stable_id()),
            name: State::new(
                props
                    .get(Key::PEER_NAME)
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            ),
        }
    }

    fn update(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::ROOM => self.room.update(*value.as_stable_id()),
            Key::PEER_NAME => self
                .name
                .update(value.as_str().unwrap_or_default().to_owned()),
            _ => false,
        }
    }
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
    peers: HashMap<PeerId, Peer>,
    public_key_to_peer: HashMap<PublicKey, PeerId>,
    rooms: Vec<Room>,
    public_key: PublicKey,
    active_room: StableId,
    log: log::Log,
    _log_handle: ContextHandle<log::Log>,
    _state_change: ws::StateListener,
    _init_request: ws::Request,
    _remote_listener: ws::Listener,
    _config_listener: ws::Listener,
    _connect_room_request: ws::Request,
    _create_room_request: ws::Request,
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
            public_key_to_peer: HashMap::new(),
            rooms: Vec::new(),
            public_key: PublicKey::ZERO,
            active_room: StableId::ZERO,
            log,
            _log_handle,
            _state_change,
            _init_request: ws::Request::new(),
            _remote_listener,
            _config_listener,
            _connect_room_request: ws::Request::new(),
            _create_room_request: ws::Request::new(),
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
        let no_room_count = self
            .peers
            .values()
            .filter(|p| *p.room == StableId::ZERO)
            .count()
            + usize::from(self.active_room == StableId::ZERO);

        html! {
            <div id="content" class="rows">
                <div class="control-group">
                    <Icon name="home" invert={true} />
                    <span>{"Rooms"}</span>
                    <div class="fill"></div>
                    <button class="btn square primary" title="Add room"
                        onclick={ctx.link().callback(|_| Msg::CreateRoom)}>
                        <Icon name="plus-circle" />
                    </button>
                </div>

                <section class="list" key="rooms-list">
                    if no_room_count > 0 {
                        <div class="list-content" key="no-room">
                            <Icon name="question-mark-circle" invert={true} />
                            <span class="list-label">{COMMON_ROOM_NAME}</span>
                            if self.active_room != StableId::ZERO {
                                <button class="btn square list-action"
                                    title="Join Common Room"
                                    onclick={ctx.link().callback(|_| Msg::Disconnect)}>
                                    <Icon name="link" />
                                </button>
                            }
                            <span class="bullet" title="Players not in a room">{no_room_count}</span>
                        </div>
                    }

                    {for self.rooms.iter().map(|room| self.view_room(ctx, room))}
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
            let name = room.name.clone();
            let onrequestdelete = ctx.props().onrequestdelete.clone();
            let onclick = Callback::from(move |_| onrequestdelete.emit((id, name.clone())));

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

        let title = if is_local {
            "Room owned by you".to_string()
        } else {
            match self
                .public_key_to_peer
                .get(&room.id.public_key)
                .and_then(|id| self.peers.get(id))
            {
                Some(peer) => format!("Room owned by '{}'", *peer.name),
                None => "Remote room".to_string(),
            }
        };

        let peer_count = self.peers.values().filter(|p| *p.room == room.id).count()
            + usize::from(self.active_room == room.id);

        let settings_button = is_local.then(|| {
            let id = room.id.id;
            let onopensettings = ctx.props().onopensettings.clone();
            let onclick = Callback::from(move |_| onopensettings.emit(id));

            html! {
                <button class="btn square list-action" {onclick} title="Room settings">
                    <Icon name="cog" />
                </button>
            }
        });

        html! {
            <div class="list-content" key={room.id}>
                <Icon name={room_icon} invert={true} title={title.clone()} />
                <span class="list-label" title={title.clone()}>{&room.name}</span>
                {connect_button}
                {settings_button}
                {delete_button}
                <span class="bullet" title="Players in this room">{peer_count}</span>
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
        if id.is_local() {
            return StableId::new(self.public_key, id.id);
        }

        let Some(peer) = self.peers.get(&id.peer_id) else {
            return StableId::ZERO;
        };

        StableId::new(peer.public_key, id.id)
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
                self.peers.clear();
                self.public_key_to_peer.clear();

                for object in body.local {
                    let id = StableId::new(self.public_key, object.id);

                    if let Some(room) = Room::from_remote(id, &object) {
                        self.rooms.push(room);
                    }
                }

                for peer in body.peers {
                    self.add_peer(peer.peer_id, peer.public_key, &peer.props);

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
                        props,
                        ..
                    } => {
                        self.add_peer(peer_id, public_key, &props);

                        for object in objects {
                            let id = StableId::new(public_key, object.id);

                            if let Some(room) = Room::from_remote(id, &object) {
                                self.rooms.push(room);
                                self.rooms.sort_by(Room::cmp);
                            }
                        }

                        Ok(true)
                    }
                    RemoteUpdateBody::PeerDisconnect { peer_id } => Ok(self.remove_peer(peer_id)),
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
                    RemoteUpdateBody::PeerUpdate {
                        peer_id,
                        key,
                        value,
                    } => Ok(self
                        .peers
                        .get_mut(&peer_id)
                        .map(|p| p.update(key, value))
                        .unwrap_or(false)),
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
                self._create_room_request = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::CreateObjectRequest {
                        ty: Type::ROOM,
                        props: api::Properties::from([(Key::OBJECT_NAME, Value::from("New Room"))]),
                    })
                    .on_packet(ctx.link().callback(Msg::CreateRoomResult))
                    .send();

                Ok(false)
            }
            Msg::CreateRoomResult(body) => {
                let body = body?;
                _ = body.decode()?;
                Ok(true)
            }
            Msg::ContextUpdate(log) => {
                self.log = log;
                Ok(false)
            }
        }
    }

    fn add_peer(
        &mut self,
        peer_id: PeerId,
        public_key: PublicKey,
        props: &api::Properties,
    ) -> bool {
        if let Some(old) = self.peers.insert(peer_id, Peer::new(public_key, props)) {
            self.public_key_to_peer.remove(&old.public_key);
        }

        self.public_key_to_peer.insert(public_key, peer_id);
        true
    }

    fn remove_peer(&mut self, peer_id: PeerId) -> bool {
        let Some(peer) = self.peers.remove(&peer_id) else {
            return false;
        };

        self.public_key_to_peer.remove(&peer.public_key);
        self.rooms.retain(|r| r.id.public_key != peer.public_key);
        true
    }
}
