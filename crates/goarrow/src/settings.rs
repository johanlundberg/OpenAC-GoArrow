//! The settings (`GoArrowSettings.cs`): one JSON document, `settings.json`
//! in the plugin's storage, for every character (as GoArrow kept it), with
//! GoArrow's member names, defaults and load-time repairs. The recall
//! places it learns are kept per character in `RecallsByCharacter`.
//!
//! GoArrow also read and then deleted its older one-file-per-setting keys;
//! this port never had them, so it does not (DIVERGENCES.md, GA-10).

use std::collections::BTreeMap;

use openac_plugin::Host;
use openac_plugin::storage::{StoragePlace, StorageRoot};
use serde::{Deserialize, Deserializer, Serialize};

use crate::finder::RouteCostProfile;

/// The settings file's key.
pub const SETTINGS_KEY: &str = "settings.json";

/// The smallest and largest zoom of the earlier map, kept in `MapZoom`
/// (the map zooms by [`Settings::map_zoom_factor`] now).
pub const MAP_ZOOM_RANGE: (f64, f64) = (1.0, 16.0);

/// Where the plugin's own files live: its data, for every character.
#[must_use]
pub fn storage() -> StoragePlace {
    StoragePlace::global(StorageRoot::PluginData)
}

fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// The recall places one character has used (`CharacterRecalls`): each
/// `"ns,ew"`, or empty.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct CharacterRecalls {
    /// The lifestone.
    #[serde(deserialize_with = "null_default")]
    pub lifestone: String,
    /// The marketplace.
    #[serde(deserialize_with = "null_default")]
    pub marketplace: String,
    /// The allegiance hometown.
    #[serde(deserialize_with = "null_default")]
    pub allegiance: String,
    /// The house.
    #[serde(deserialize_with = "null_default")]
    pub house: String,
    /// The mansion.
    #[serde(deserialize_with = "null_default")]
    pub mansion: String,
}

/// The settings document, member for member as GoArrow writes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Settings {
    /// The destination's name, restored by name at start.
    #[serde(deserialize_with = "null_default")]
    pub destination_name: String,
    /// Go also walks.
    pub auto_navigate: bool,
    /// Re-plan while standing or walking without a walk under way.
    pub recalculate_route: bool,
    /// The window opens at start.
    pub panel_visible: bool,
    /// The arrow shows the distance.
    pub show_distance: bool,
    /// The arrow shows the bearing.
    pub show_bearing: bool,
    /// The arrow shows.
    pub hud_visible: bool,
    /// GoArrow's separate Stop/Resume toolbar showed; kept in the file,
    /// the arrow carries its buttons now (DIVERGENCES.md, GA-14).
    pub toolbar_visible: bool,
    /// Which repair of hidden overlays the file has had (saved as 1).
    pub visibility_settings_version: i32,
    /// The overlays let clicks through to the game.
    pub hud_click_through: bool,
    /// Kept in the file; GoArrow never read it.
    pub hud_scale: f64,
    /// The arrow's offset from the screen's top-right corner, pixels.
    pub arrow_offset_x: f64,
    /// The arrow's offset, pixels.
    pub arrow_offset_y: f64,
    /// The toolbar's offset (kept in the file).
    pub toolbar_offset_x: f64,
    /// The toolbar's offset (kept in the file).
    pub toolbar_offset_y: f64,
    /// The map's offset from the screen's top-left corner, pixels (it was
    /// the dungeon map's; DIVERGENCES.md, GA-15).
    pub dungeon_offset_x: f64,
    /// The map's offset, pixels.
    pub dungeon_offset_y: f64,
    /// Which reset of overlay positions the file has had (saved as 1).
    pub overlay_position_version: i32,
    /// The map shows.
    pub map_visible: bool,
    /// The map turns to the dungeon's floorplan indoors.
    pub dungeon_map_visible: bool,
    /// The middle of the Dereth map, east-west, map units.
    pub map_center_east_west: f64,
    /// The middle of the Dereth map, north-south, map units.
    pub map_center_north_south: f64,
    /// Kept in the file; the map zooms by [`Settings::map_zoom`].
    pub map_width: f64,
    /// Kept in the file.
    pub map_height: f64,
    /// How close a walk to a waypoint stops, metres.
    pub arrival_distance: f64,
    /// Walking is allowed at all; off, Go only plans.
    pub use_navigation_automation: bool,
    /// Set by `/go lock`; GoArrow never read it.
    pub navigation_locked: bool,
    /// The route cost profile's number (0 to 3).
    pub route_cost_profile: i32,
    /// Kept in the file; GoArrow never read it.
    pub max_navigation_retries: i32,
    /// Kept in the file; GoArrow never read it.
    pub interaction_timeout_seconds: f64,
    /// How old a cached Atlas download may be to stand in for a failed
    /// one, days (0: any age).
    pub atlas_cache_max_age_days: i32,
    /// Where `/go update` downloads location data from.
    #[serde(deserialize_with = "null_default")]
    pub external_data_url: String,
    /// Download the location data when the plugin starts with none
    /// cached (DIVERGENCES.md, GA-70).
    pub download_data_at_start: bool,
    /// GoArrow's dungeon map download; kept in the file (DIVERGENCES.md,
    /// GA-16).
    #[serde(deserialize_with = "null_default")]
    pub dungeon_map_url: String,
    /// The lifestone recall's place, `"ns,ew"`.
    #[serde(deserialize_with = "null_default")]
    pub last_portal_recall: String,
    /// The marketplace recall's place.
    #[serde(deserialize_with = "null_default")]
    pub last_secondary_recall: String,
    /// The allegiance recall's place.
    #[serde(deserialize_with = "null_default")]
    pub last_allegiance_recall: String,
    /// The house recall's place.
    #[serde(deserialize_with = "null_default")]
    pub last_house_recall: String,
    /// The mansion recall's place.
    #[serde(deserialize_with = "null_default")]
    pub last_mansion_recall: String,
    /// The recall places by `"{character id}/{world}"`.
    #[serde(deserialize_with = "null_default")]
    pub recalls_by_character: BTreeMap<String, CharacterRecalls>,
    /// Favourite destinations, by name.
    #[serde(deserialize_with = "null_default")]
    pub favorite_destinations: Vec<String>,
    /// The route is drawn on the ground (new; DIVERGENCES.md, GA-15).
    pub route_line_visible: bool,
    /// The earlier map's zoom, 1 to 16 (new; DIVERGENCES.md, GA-15): read
    /// once into [`Settings::map_zoom_factor`] when that is not set yet.
    pub map_zoom: f64,
    /// The map's zoom factor, the original GoArrow map's (the zoom is √2 to
    /// its power; DIVERGENCES.md, GA-61); none until the map first saves
    /// it.
    pub map_zoom_factor: Option<f64>,
    /// The map shows every kind of place at every zoom (the original's
    /// "Show locations at all zoom levels"; DIVERGENCES.md, GA-63).
    pub show_locations_all_zooms: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            destination_name: String::new(),
            auto_navigate: false,
            recalculate_route: true,
            panel_visible: true,
            show_distance: true,
            show_bearing: true,
            hud_visible: true,
            toolbar_visible: true,
            visibility_settings_version: 0,
            hud_click_through: false,
            hud_scale: 1.0,
            arrow_offset_x: DEFAULT_ARROW_OFFSET.0,
            arrow_offset_y: DEFAULT_ARROW_OFFSET.1,
            toolbar_offset_x: DEFAULT_TOOLBAR_OFFSET.0,
            toolbar_offset_y: DEFAULT_TOOLBAR_OFFSET.1,
            dungeon_offset_x: DEFAULT_MAP_OFFSET.0,
            dungeon_offset_y: DEFAULT_MAP_OFFSET.1,
            overlay_position_version: 0,
            map_visible: false,
            dungeon_map_visible: true,
            map_center_east_west: 0.0,
            map_center_north_south: 0.0,
            map_width: 1000.0,
            map_height: 1000.0,
            arrival_distance: 0.5,
            use_navigation_automation: true,
            navigation_locked: false,
            route_cost_profile: 0,
            max_navigation_retries: 2,
            interaction_timeout_seconds: 15.0,
            atlas_cache_max_age_days: 30,
            external_data_url: String::new(),
            download_data_at_start: true,
            dungeon_map_url: String::new(),
            last_portal_recall: String::new(),
            last_secondary_recall: String::new(),
            last_allegiance_recall: String::new(),
            last_house_recall: String::new(),
            last_mansion_recall: String::new(),
            recalls_by_character: BTreeMap::new(),
            favorite_destinations: Vec::new(),
            route_line_visible: true,
            map_zoom: 1.0,
            map_zoom_factor: None,
            show_locations_all_zooms: false,
        }
    }
}

/// The arrow's place at first start: 70 px in from the right, 120 down.
pub const DEFAULT_ARROW_OFFSET: (f64, f64) = (-70.0, 120.0);
/// The toolbar's place GoArrow used.
pub const DEFAULT_TOOLBAR_OFFSET: (f64, f64) = (-70.0, 215.0);
/// The map's place at first start: 25 px in from the left, 95 down.
pub const DEFAULT_MAP_OFFSET: (f64, f64) = (25.0, 95.0);

fn positive_or(value: f64, fallback: f64) -> f64 {
    if value > 0.0 { value } else { fallback }
}

impl Settings {
    /// Reads a settings document and repairs it as GoArrow's `Load` did.
    ///
    /// # Errors
    /// The text is not a settings document.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut settings: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        settings.sanitize();
        Ok(settings)
    }

    /// The document as GoArrow's `Save` writes it: indented, both repair
    /// versions set to 1.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut saved = self.clone();
        saved.overlay_position_version = 1;
        saved.visibility_settings_version = 1;
        serde_json::to_string_pretty(&saved).unwrap_or_default()
    }

    /// GoArrow's load-time repairs: unusable numbers back to their
    /// defaults, overlay positions from before version 1 reset, and a
    /// profile that hid everything shown again.
    pub fn sanitize(&mut self) {
        self.hud_scale = positive_or(self.hud_scale, 1.0);
        if self.overlay_position_version < 1 {
            self.reset_overlay_positions();
        }
        self.map_width = positive_or(self.map_width, 1000.0);
        self.map_height = positive_or(self.map_height, 1000.0);
        self.arrival_distance = positive_or(self.arrival_distance, 0.5);
        self.max_navigation_retries = self.max_navigation_retries.max(0);
        self.interaction_timeout_seconds = positive_or(self.interaction_timeout_seconds, 15.0);
        self.atlas_cache_max_age_days = self.atlas_cache_max_age_days.max(0);
        if RouteCostProfile::from_index(self.route_cost_profile).is_none() {
            self.route_cost_profile = 0;
        }
        self.map_zoom = if self.map_zoom.is_finite() {
            self.map_zoom.clamp(MAP_ZOOM_RANGE.0, MAP_ZOOM_RANGE.1)
        } else {
            1.0
        };
        self.map_zoom_factor = self
            .map_zoom_factor
            .filter(|f| !f.is_nan())
            .map(|f| f.min(crate::map_view::MAX_ZOOM_FACTOR));
        for value in [
            &mut self.map_center_east_west,
            &mut self.map_center_north_south,
        ] {
            if !value.is_finite() {
                *value = 0.0;
            }
        }
        if self.visibility_settings_version < 1 {
            self.restore_ui_hidden_by_old_defaults();
        }
    }

    /// The overlays back to where they start.
    pub fn reset_overlay_positions(&mut self) {
        (self.arrow_offset_x, self.arrow_offset_y) = DEFAULT_ARROW_OFFSET;
        (self.toolbar_offset_x, self.toolbar_offset_y) = DEFAULT_TOOLBAR_OFFSET;
        (self.dungeon_offset_x, self.dungeon_offset_y) = DEFAULT_MAP_OFFSET;
        self.overlay_position_version = 1;
    }

    /// GoArrow's `RestoreUiHiddenByOldDefaults`: earlier builds saved every
    /// overlay hidden; the window and the arrow come back.
    fn restore_ui_hidden_by_old_defaults(&mut self) {
        if !self.panel_visible && !self.hud_visible && !self.toolbar_visible && !self.map_visible {
            self.panel_visible = true;
        }
        if !self.hud_visible && !self.toolbar_visible && !self.map_visible {
            if !self.show_distance
                && !self.show_bearing
                && !self.recalculate_route
                && !self.use_navigation_automation
            {
                self.show_distance = true;
                self.show_bearing = true;
                self.recalculate_route = true;
                self.use_navigation_automation = true;
            }
            self.hud_visible = true;
        }
    }

    /// The route cost profile.
    #[must_use]
    pub fn profile(&self) -> RouteCostProfile {
        RouteCostProfile::from_index(self.route_cost_profile).unwrap_or_default()
    }

    /// Whether a name is a favourite (ignoring case).
    #[must_use]
    pub fn is_favorite(&self, name: &str) -> bool {
        self.favorite_destinations
            .iter()
            .any(|f| crate::text::eq_ignore_case(f, name))
    }

    /// Adds a favourite unless it is one (ignoring case); true when added.
    pub fn add_favorite(&mut self, name: &str) -> bool {
        if self.is_favorite(name) {
            return false;
        }
        self.favorite_destinations.push(name.to_owned());
        true
    }

    /// Removes a favourite (ignoring case); true when it was one.
    pub fn remove_favorite(&mut self, name: &str) -> bool {
        let before = self.favorite_destinations.len();
        self.favorite_destinations
            .retain(|f| !crate::text::eq_ignore_case(f, name));
        before != self.favorite_destinations.len()
    }
}

/// Reads the settings, defaults when there are none or they do not read
/// (logged).
pub fn load(host: &mut Host<'_>) -> Settings {
    match host.storage_read_text(&storage(), SETTINGS_KEY) {
        Ok(Some(text)) => Settings::from_json(&text).unwrap_or_else(|error| {
            host.warn(&format!(
                "GoArrow: {SETTINGS_KEY} could not be read ({error}); using the defaults."
            ));
            let mut settings = Settings::default();
            settings.sanitize();
            settings
        }),
        Ok(None) => {
            let mut settings = Settings::default();
            settings.sanitize();
            settings
        }
        Err(error) => {
            host.warn(&format!(
                "GoArrow: {SETTINGS_KEY} could not be read: {error}"
            ));
            let mut settings = Settings::default();
            settings.sanitize();
            settings
        }
    }
}

/// Writes the settings; a failure is logged.
pub fn save(host: &mut Host<'_>, settings: &Settings) {
    if let Err(error) = host.storage_write_text(&storage(), SETTINGS_KEY, &settings.to_json()) {
        host.warn(&format!("GoArrow: Could not save {SETTINGS_KEY}: {error}"));
    }
}
