//! GoArrow for openAC-rust: the GoArrow navigation plugin as a WebAssembly
//! plugin. It finds a route to a named place or to map coordinates over
//! walks, portals and recalls, points the way, and can walk it.
//!
//! This crate is the plugin: a `cdylib` built for `wasm32-unknown-unknown`
//! with the openAC plugin SDK, and an `rlib` the native tests link. The
//! route-finding core is pure and host-free:
//!
//! - [`coords`]: map coordinates, their text form and parser;
//! - [`distance`]: distances as the player reads them;
//! - [`location`]: a place and the four ways GoArrow reads one;
//! - [`loading`]: reading a location file or Atlas data in steps;
//! - [`database`]: the loaded places, portal devices, route starts and
//!   corrections; the Atlas download check;
//! - [`corrections`]: the route corrections file;
//! - [`route`]: a route and its steps;
//! - [`graph`]: the route graph and its searches;
//! - [`finder`]: the best route, and its alternatives;
//! - [`destination`]: the destination, its route and the arrow's guidance;
//! - [`portal_match`]: how a live portal is matched to a route's place;
//! - [`traversals`]: learned dungeon paths;
//! - [`ace_portals`]: the portal table (where every portal stands and
//!   leads), read beside the plugin.
//!
//! Over the client:
//!
//! - [`navigator`]: walks the route;
//! - [`plugin`]: [`GoArrow`], the plugin: lifecycle, destinations, route
//!   work, location data, corrections, recalls and marks;
//! - [`commands`]: the `/go` commands;
//! - [`settings`]: the settings file;
//! - [`data`]: where the data lives, the download check, load jobs;
//! - [`window`]: the window (`markup/goarrow.xml`) and its bindings;
//! - [`hud`]: the arrow overlay;
//! - [`shapes`]: smooth shapes drawn into coverage masks (pure);
//! - [`sprites`]: those shapes as images the client holds;
//! - [`map`]: the map overlay (Dereth and the dungeon floorplan);
//! - [`map_view`]: the Dereth view's rules, the original GoArrow map's (pure);
//! - [`map_icons`]: the original GoArrow's map icons, as images;
//! - [`terrain_map`]: the sharp map of Dereth drawn from the terrain;
//! - [`ground`]: the route drawn on the ground;
//! - [`timestamp`]: the cache's dates;
//! - [`dungeon_search`]: the search for a portal inside a dungeon;
//! - [`indoor_preview`]: the planned indoor path the window lists.

pub mod ace_portals;
pub mod commands;
pub mod coords;
pub mod corrections;
pub mod data;
pub mod database;
pub mod destination;
pub mod distance;
pub mod dungeon_search;
pub mod finder;
pub mod graph;
pub mod ground;
pub mod hud;
pub mod indoor_preview;
pub mod loading;
pub mod location;
pub mod map;
pub mod map_icons;
pub mod map_view;
pub mod navigator;
pub mod plugin;
pub mod portal_match;
pub mod route;
pub mod settings;
pub mod shapes;
pub mod sprites;
pub mod terrain_map;
pub mod timestamp;
pub mod traversals;
pub mod window;

mod text;
mod xml;

pub use plugin::GoArrow;

openac_plugin::export_plugin!(GoArrow);
