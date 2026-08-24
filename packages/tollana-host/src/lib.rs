pub mod ai;
pub mod clock;
pub mod context;
pub mod error;
pub mod goal;
pub mod host;
pub mod plugin;
pub mod schema;

pub use ai::{Ai, METHOD_CHAT, METHOD_EMBED, METHOD_GENERATE};
pub use clock::{Clock, METHOD_NOW_MONOTONIC, METHOD_NOW_WALL};
pub use context::{Context, METHOD_LIST, METHOD_READ};
pub use error::HostError;
pub use goal::{Goal, Railguards, METHOD_CANCEL, METHOD_JOIN, METHOD_SPAWN};
pub use host::Host;
pub use plugin::{Plugin, PluginContext, PluginResult};
pub use schema::{
    parse_package_schema, MethodSchema, PackageSchema, AI_SCHEMA_BYTES, CLOCK_SCHEMA_BYTES,
    CONTEXT_SCHEMA_BYTES, GOAL_SCHEMA_BYTES,
};
