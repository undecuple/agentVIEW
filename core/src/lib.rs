#![deny(unsafe_code)]
//! Read-only viewer core. Pure functions: a request path in, a response out. No network, no writes.
pub mod gate;
pub mod html;
pub mod images;
pub mod markdown;
pub mod policy;
pub mod router;
pub mod sys;

pub use policy::{Config, Refused};
pub use router::{handle, Response};
