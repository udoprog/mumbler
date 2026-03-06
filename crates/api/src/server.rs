use musli_core::{Decode, Encode};
use musli_web::api;

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Header {
    /// The type of the request.
    pub request: u16,
    /// The type id of the error message.
    pub error: u16,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ConnectRequest {
    /// The protocol version of the client.
    pub version: u32,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ServerHello {
    /// The protocol version of the server.
    pub version: u32,
}

api::define! {
    pub type Connect;

    impl Endpoint for Connect {
        impl Request for ConnectRequest;
        type Response<'de> = ServerHello;
    }
}
