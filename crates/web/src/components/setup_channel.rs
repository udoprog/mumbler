use core::cell::RefCell;
use std::rc::{Rc, Weak};

use musli_web::web03::prelude::*;
use yew::prelude::*;

use crate::error::Error;

struct Inner {
    onchannel: Callback<Result<ws::Channel, Error>>,
    _channel: ws::Request,
    _state_change: ws::StateListener,
    state: ws::State,
    ws: ws::Handle,
    on_channel: Callback<Result<ws::Channel, ws::Error>>,
    on_state: Callback<ws::State>,
}

pub(crate) struct SetupChannel {
    _inner: Rc<RefCell<Inner>>,
}

impl SetupChannel {
    /// Construct a new channel builder.
    pub(crate) fn new<T>(ctx: &Context<T>, onchannel: Callback<Result<ws::Channel, Error>>) -> Self
    where
        T: Component,
    {
        let this = Self {
            _inner: Rc::new_cyclic(|inner: &Weak<RefCell<Inner>>| {
                let on_ws = Callback::from({
                    let inner = inner.clone();

                    move |ws| {
                        let Some(inner) = inner.upgrade() else {
                            return;
                        };

                        inner.borrow_mut().on_ws(ws);
                    }
                });

                let on_state = Callback::from({
                    let inner = inner.clone();

                    move |state| {
                        let Some(inner) = inner.upgrade() else {
                            return;
                        };

                        inner.borrow_mut().on_state(state);
                    }
                });

                let (ws, _ws_handle) = ctx
                    .link()
                    .context::<ws::Handle>(on_ws)
                    .expect("WebSocket context not found");

                let on_channel = Callback::from({
                    let inner = inner.clone();

                    move |channel| {
                        let Some(inner) = inner.upgrade() else {
                            return;
                        };

                        inner.borrow_mut().on_channel(channel);
                    }
                });

                RefCell::new(Inner {
                    onchannel,
                    _channel: ws::Request::default(),
                    _state_change: ws::StateListener::default(),
                    state: ws::State::Closed,
                    ws,
                    on_channel,
                    on_state,
                })
            }),
        };

        this._inner.borrow_mut().setup();
        this
    }
}

impl Inner {
    fn on_ws(&mut self, ws: ws::Handle) {
        self.ws = ws;
        self.setup();
    }

    fn on_state(&mut self, state: ws::State) {
        self.state = state;
        self.refresh();
    }

    fn on_channel(&mut self, channel: Result<ws::Channel, ws::Error>) {
        match channel {
            Ok(channel) => {
                self.onchannel.emit(Ok(channel));
            }
            Err(error) => {
                self.onchannel.emit(Err(error.into()));
            }
        }
    }

    fn setup(&mut self) {
        let (state, _state_change) = self.ws.on_state_change(self.on_state.clone());
        self.state = state;
        self._state_change = _state_change;
        self.refresh();
    }

    fn refresh(&mut self) {
        if self.state.is_open() {
            self._channel = self.ws.channel().on_open(self.on_channel.clone()).send();
        } else {
            self.onchannel.emit(Ok(ws::Channel::default()));
        }
    }
}
