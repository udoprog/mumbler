use musli_core::{Decode, Encode};
use musli_web::api;

#[derive(Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Empty;

/// Event emitted when the API is initialized.
#[derive(Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeEvent {
    /// The name of the current user.
    pub name: Option<String>,
}

api::define! {
    pub type Initialize;

    impl Endpoint for Initialize {
        impl Request for Empty;
        type Response<'de> = InitializeEvent;
    }
}
