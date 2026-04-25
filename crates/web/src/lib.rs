#![allow(clippy::type_complexity)]
#![allow(clippy::single_match)]
#![allow(clippy::vec_init_then_push)]
#![allow(clippy::collapsible_match)]

mod components;
mod consts;
mod drag_over;
mod error;
mod images;
mod log;
mod objects;
mod order;
mod peers;
mod state;

pub use self::components::App;
