//! V1 domain vocabulary. Pure data + the event catalogue; no storage, no IO,
//! no model calls. Everything else depends on this crate, never the reverse.

pub mod agent;
pub mod baseline;
pub mod entities;
pub mod event_catalog;
pub mod events;
pub mod ids;
pub mod output;
pub mod record;

pub use agent::*;
pub use baseline::*;
pub use entities::*;
pub use events::*;
pub use ids::{content_hash, new_id, normalize_for_hash, now};
pub use output::*;
pub use record::*;
