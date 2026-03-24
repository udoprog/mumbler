use api::RemoteId;
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use web_sys::MouseEvent;
use yew::prelude::*;

use crate::error::Error;
use crate::log;

use super::{Icon, RoomSettings, Rooms};

pub(crate) enum Msg {
    OpenSettings(RemoteId),
    CloseSettings,
    RequestDelete(RemoteId, String),
    CancelDelete,
    ConfirmDelete(RemoteId),
    DeleteResult(Result<Packet<api::RemoveObject>, ws::Error>),
    ContextUpdate(log::Log),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

pub(crate) struct RoomsPage {
    open_settings: Option<RemoteId>,
    confirm_delete: Option<(RemoteId, String)>,
    log: log::Log,
    _log_handle: ContextHandle<log::Log>,
    _delete_request: ws::Request,
}

impl Component for RoomsPage {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _log_handle) = ctx
            .link()
            .context::<log::Log>(ctx.link().callback(Msg::ContextUpdate))
            .expect("log::Log context not found");

        Self {
            open_settings: None,
            confirm_delete: None,
            log,
            _log_handle,
            _delete_request: ws::Request::new(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::OpenSettings(id) => {
                self.open_settings = Some(id);
                true
            }
            Msg::CloseSettings => {
                self.open_settings = None;
                true
            }
            Msg::RequestDelete(id, name) => {
                self.confirm_delete = Some((id, name));
                true
            }
            Msg::CancelDelete => {
                self.confirm_delete = None;
                true
            }
            Msg::ConfirmDelete(id) => {
                self.confirm_delete = None;

                self._delete_request = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::RemoveObjectRequest { id: id.id })
                    .on_packet(ctx.link().callback(Msg::DeleteResult))
                    .send();

                true
            }
            Msg::DeleteResult(result) => {
                if let Err(e) = result.and_then(|p| p.decode()) {
                    self.log.error("rooms_page", Error::from(e));
                }
                false
            }
            Msg::ContextUpdate(log) => {
                self.log = log;
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let ws = ctx.props().ws.clone();

        html! {
            <>
                <Rooms
                    {ws}
                    onopensettings={ctx.link().callback(Msg::OpenSettings)}
                    onrequestdelete={ctx.link().callback(|(id, name)| Msg::RequestDelete(id, name))}
                />

                if let Some((id, ref name)) = self.confirm_delete {
                    <div class="modal-backdrop" onclick={ctx.link().callback(|_| Msg::CancelDelete)}>
                        <div class="modal" onclick={|ev: MouseEvent| ev.stop_propagation()}>
                            <div class="modal-header">
                                <h2>{"Confirm Deletion"}</h2>
                                <button class="btn sm square danger" title="Cancel"
                                    onclick={ctx.link().callback(|_| Msg::CancelDelete)}>
                                    <Icon name="x-mark" />
                                </button>
                            </div>
                            <div class="modal-body rows">
                                <p>{format!("Remove \"{}\"?", name)}</p>
                                <div class="btn-group">
                                    <button class="btn danger"
                                        onclick={ctx.link().callback(move |_| Msg::ConfirmDelete(id))}>
                                        {"Delete"}
                                    </button>
                                    <button class="btn"
                                        onclick={ctx.link().callback(|_| Msg::CancelDelete)}>
                                        {"Cancel"}
                                    </button>
                                </div>
                            </div>
                        </div>
                    </div>
                }

                if let Some(id) = self.open_settings {
                    <div class="modal-backdrop" onclick={ctx.link().callback(|_| Msg::CloseSettings)}>
                        <div class="modal" onclick={|ev: MouseEvent| ev.stop_propagation()}>
                            <div class="modal-header">
                                <h2>{"Room Settings"}</h2>
                                <button class="btn sm square danger" title="Close"
                                    onclick={ctx.link().callback(|_| Msg::CloseSettings)}>
                                    <Icon name="x-mark" />
                                </button>
                            </div>
                            <div class="modal-body">
                                <RoomSettings ws={ctx.props().ws.clone()} {id} />
                            </div>
                        </div>
                    </div>
                }
            </>
        }
    }
}
