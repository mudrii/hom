//! # hom-web
//!
//! WebSocket server for HOM. Serves a live browser view of all panes.
//! Start with `hom --web` (default port 4242) or `hom --web-port 8080`.

pub mod frame;
pub mod server;
pub mod viewer;

pub use frame::{WebCell, WebFrame, WebInput, WebPane};
pub use server::WebServer;
