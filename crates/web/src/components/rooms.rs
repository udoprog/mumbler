use core::cmp::Ordering;

use api::{Key, PeerId, RemoteId, StableId, Type, UpdateBody, Value};
use api::{RemoteObject, RemoteUpdateBody};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use yew::prelude::*;

use crate::error::Error;
use crate::log;
use crate::peers::Peers;

use super::{COMMON_ROOM, Icon};

pub(crate) enum Msg {
    StateChanged(ws::State),
    Channel(Result<ws::Channel, ws::Error>),
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
    pub(crate) onopensettings: Callback<RemoteId>,
    pub(crate) onrequestdelete: Callback<RemoteId>,
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
    peers: Peers,
    rooms: Vec<Room>,
    active_room: StableId,
    log: log::Log,
    _log_handle: ContextHandle<log::Log>,
    _state_change: ws::StateListener,
    _init_request: ws::Request,
    _remote_listener: ws::Listener,
    _config_listener: ws::Listener,
    _connect_room_request: ws::Request,
    _create_room_request: ws::Request,
    _channel: ws::Request,
    channel: ws::Channel,
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
            peers: Peers::default(),
            rooms: Vec::new(),
            active_room: StableId::ZERO,
            log,
            _log_handle,
            _state_change,
            _init_request: ws::Request::new(),
            _remote_listener,
            _config_listener,
            _connect_room_request: ws::Request::new(),
            _create_room_request: ws::Request::new(),
            _channel: ws::Request::new(),
            channel: ws::Channel::default(),
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
            .iter()
            .filter(|p| *p.props.get(Key::ROOM).as_stable_id() == StableId::ZERO)
            .count()
            + usize::from(self.active_room == StableId::ZERO);

        let is_no_room_active = self.active_room == StableId::ZERO;

        let no_room_class = classes! {
            "list-content",
            is_no_room_active.then_some("selected"),
        };

        let on_no_room_click =
            (!is_no_room_active).then(|| ctx.link().callback(|_| Msg::Disconnect));

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
                    <div class={no_room_class} key="no-room" onclick={on_no_room_click}>
                        <Icon name="question-mark-circle" invert={true} />
                        <span class="list-label">{COMMON_ROOM}</span>
                        <span class="bullet" title="Players not in a room">{no_room_count}</span>
                    </div>

                    {for self.rooms.iter().map(|room| self.view_room(ctx, room))}
                </section>
            </div>
        }
    }
}

impl Rooms {
    fn view_room(&self, ctx: &Context<Self>, room: &Room) -> Html {
        let is_active = self.active_room == room.id;
        let is_local = room.id.public_key == self.peers.public_key;

        let remove_button = is_local.then(|| {
            let id = self.peers.to_remote_id(&room.id);

            let onclick = ctx.props().onrequestdelete.reform(move |ev: MouseEvent| {
                ev.stop_propagation();
                id
            });

            html! {
                <button class="btn square list-action" {onclick} title="Remove room">
                    <Icon name="trash" />
                </button>
            }
        });

        let room_icon = if is_local { "home" } else { "home-modern" };

        let owner = if is_local {
            Some("you".to_string())
        } else {
            self.peers
                .by_public_key(&room.id.public_key)
                .map(|peer| peer.display())
        };

        let title = if is_local {
            "Room owned by you".to_string()
        } else {
            match self.peers.by_public_key(&room.id.public_key) {
                Some(peer) => format!("Room owned by '{}'", peer.display()),
                None => "Remote room".to_string(),
            }
        };

        let peer_count = self
            .peers
            .iter()
            .filter(|p| *p.props.get(Key::ROOM).as_stable_id() == room.id)
            .count()
            + usize::from(self.active_room == room.id);

        let settings_button = is_local.then(|| {
            let id = self.peers.to_remote_id(&room.id);

            let onopensettings = ctx.props().onopensettings.clone();

            let onclick = Callback::from(move |ev: MouseEvent| {
                ev.stop_propagation();
                onopensettings.emit(id);
            });

            html! {
                <button class="btn square list-action" {onclick} title="Room settings">
                    <Icon name="cog" />
                </button>
            }
        });

        let room_id = room.id;

        let on_row_click =
            (!is_active).then(|| ctx.link().callback(move |_| Msg::Connect(room_id)));

        let row_class = classes! {
            "list-content",
            is_active.then_some("selected"),
        };

        html! {
            <div class={row_class} key={room.id} onclick={on_row_click}>
                <Icon name={room_icon} invert={true} title={title.clone()} />
                <span class="list-label" title={title.clone()}>
                    <span>{&room.name}</span>

                    if let Some(owner) = &owner {
                        <span class="sublabel">{owner.clone()}</span>
                    }
                </span>
                {settings_button}
                {remove_button}
                <span class="bullet" title="Players in this room">{peer_count}</span>
            </div>
        }
    }

    fn refresh(&mut self, ctx: &Context<Self>) {
        if self.state.is_open() {
            self._channel = ctx
                .props()
                .ws
                .channel()
                .on_open(ctx.link().callback(Msg::Channel))
                .send();
        } else {
            self.peers = Peers::default();
            self.active_room = StableId::ZERO;
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
            Msg::Channel(channel) => {
                self.channel = channel?;

                self._init_request = self
                    .channel
                    .request()
                    .body(api::InitializeRoomsRequest)
                    .on_packet(ctx.link().callback(Msg::Initialized))
                    .send();

                Ok(true)
            }
            Msg::Initialized(body) => {
                let body = body?;
                let body = body.decode()?;

                self.peers.public_key = body.public_key;
                self.active_room = *body.props.get(Key::ROOM).as_stable_id();
                self.rooms.clear();
                self.peers.clear();

                for object in body.local {
                    let id = StableId::new(self.peers.public_key, object.id);

                    if let Some(room) = Room::from_remote(id, &object) {
                        self.rooms.push(room);
                    }
                }

                for peer in body.peers {
                    self.peers.insert(peer, &self.active_room);
                }

                for (peer_id, object) in body.peer_objects {
                    let id = RemoteId::new(peer_id, object.id);
                    let id = self.peers.to_stable_id(&id);

                    if let Some(room) = Room::from_remote(id, &object) {
                        self.rooms.push(room);
                    }
                }

                self.rooms.sort_by(Room::cmp);
                Ok(true)
            }
            Msg::RemoteUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                tracing::debug!(?body);

                match body {
                    RemoteUpdateBody::RemoteLost => {
                        self.rooms
                            .retain(|r| r.id.public_key != self.peers.public_key);
                        Ok(true)
                    }
                    RemoteUpdateBody::PeerConnected { peer, .. } => {
                        self.peers.insert(peer, &self.active_room);
                        Ok(true)
                    }
                    RemoteUpdateBody::PeerDisconnect { peer_id } => Ok(self.remove_peer(peer_id)),
                    RemoteUpdateBody::ObjectCreated { id, object, .. } => {
                        let id = self.peers.to_stable_id(&id);

                        if let Some(room) = Room::from_remote(id, &object) {
                            self.rooms.push(room);
                            self.rooms.sort_by(Room::cmp);
                            Ok(true)
                        } else {
                            Ok(false)
                        }
                    }
                    RemoteUpdateBody::ObjectRemoved { channel, id } => {
                        if self.channel.id() == channel {
                            return Ok(false);
                        }

                        let id = self.peers.to_stable_id(&id);

                        let prev = self.rooms.len();
                        self.rooms.retain(|r| r.id != id);
                        Ok(self.rooms.len() != prev)
                    }
                    RemoteUpdateBody::ObjectUpdated {
                        channel,
                        id,
                        key,
                        value,
                    } => {
                        if self.channel.id() == channel {
                            return Ok(false);
                        }

                        let id = self.peers.to_stable_id(&id);

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
                    } => {
                        let Some(peer) = self.peers.get_mut(peer_id) else {
                            return Ok(false);
                        };

                        peer.update(key, value, &self.active_room);
                        Ok(matches!(key, Key::ROOM | Key::PEER_NAME))
                    }
                    _ => Ok(false),
                }
            }
            Msg::ConfigUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                match body {
                    UpdateBody::Config {
                        channel,
                        key,
                        value,
                    } => {
                        if self.channel.id() == channel {
                            return Ok(false);
                        }

                        match key {
                            Key::ROOM => {
                                self.active_room = *value.as_stable_id();
                                Ok(true)
                            }
                            _ => Ok(false),
                        }
                    }
                    UpdateBody::PublicKey { public_key } => {
                        self.peers.public_key = public_key;
                        Ok(true)
                    }
                }
            }
            Msg::Disconnect => {
                let values = vec![(Key::ROOM, Value::empty())];
                self.active_room = StableId::ZERO;

                self._connect_room_request = self
                    .channel
                    .request()
                    .body(api::UpdatesRequest { values })
                    .on_packet(ctx.link().callback(Msg::ConnectResult))
                    .send();

                Ok(true)
            }
            Msg::Connect(room) => {
                let values = vec![(Key::ROOM, Value::from(room))];
                self.active_room = room;

                self._connect_room_request = self
                    .channel
                    .request()
                    .body(api::UpdatesRequest { values })
                    .on_packet(ctx.link().callback(Msg::ConnectResult))
                    .send();

                Ok(true)
            }
            Msg::ConnectResult(body) => {
                let body = body?;
                _ = body.decode()?;
                Ok(false)
            }
            Msg::CreateRoom => {
                self._create_room_request = self
                    .channel
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

    fn remove_peer(&mut self, peer_id: PeerId) -> bool {
        let Some(public_key) = self.peers.get(peer_id).map(|p| p.public_key) else {
            return false;
        };

        self.peers.remove_peer(peer_id);
        self.rooms.retain(|r| r.id.public_key != public_key);
        true
    }
}
