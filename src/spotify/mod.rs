//! Spotify Web API client: PKCE login, token storage, playback and library.
//! A Rust port of `bin/spotify-bridge` from omarchy-spotify-client.

mod api;
mod auth;
mod error;
mod http;
mod library;
mod store;
mod types;

pub use api::{ListPage, Session, Spotify};
pub use error::Error;
pub use library::{Entry, EntryKind, SearchGroup};
pub use store::{Store, StoredAuth};
pub use types::{Device, Item, ItemKind, PlayerState, Repeat, User};
