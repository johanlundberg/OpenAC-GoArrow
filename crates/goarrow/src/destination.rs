//! The destination and its guidance (`GoArrowDestination.cs`): what the
//! player is going to (a named place, a map point, a world object), the
//! current route there, the alternatives the route window cycles through,
//! and the arrow's numbers: straight-line distance to the destination,
//! and distance and bearing to the next waypoint.
//!
//! Positions come in as plain values; nothing here asks the client.

use std::rc::Rc;

use crate::coords::{Coordinates, METRES_PER_MAP_UNIT, NavPosition};
use crate::corrections::BlockedStep;
use crate::database::LocationDatabase;
use crate::finder::{MAX_ALTERNATIVES, RouteCostProfile, RouteFinder};
use crate::location::{IndoorPosition, Location};
use crate::route::{Route, RouteStep, RouteStepKind};
use crate::text;

/// Where a destination came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DestinationKind {
    /// A named place.
    #[default]
    Location,
    /// A map point.
    Coordinates,
    /// A world object.
    Object,
    /// A route.
    Route,
    /// A recall.
    Recall,
}

/// The distance, metres, within which an origin counts as the same for
/// the alternatives, and within which Go keeps a chosen alternative.
pub const SAME_ORIGIN_METRES: f64 = 20.0;

/// A way out of a dungeon known from the portal table
/// ([`Destination::try_plan_dungeon_exit_with`]).
#[derive(Debug, Clone, PartialEq)]
pub struct KnownExit {
    /// The portal's name.
    pub name: String,
    /// What the route's arrival names it by (`portal table 7000ABCD`).
    pub id: String,
    /// Where it leads.
    pub arrival: Coordinates,
}

/// What cycling the alternatives needs ([`Destination::prepare_cycle`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CyclePlan {
    /// Cycling is not possible (no destination or route, or a direction
    /// other than -1 or 1).
    Refused,
    /// The known alternatives are enough.
    Move,
    /// This many alternatives must be found first.
    Fetch(usize),
}

/// The name a map point gets as a destination when no text names it (a
/// chat link, a map click, the current location): its standard text,
/// `12.3S, 4.0W` (DIVERGENCES.md, GA-2).
#[must_use]
pub fn coordinate_destination_name(ns: f64, ew: f64) -> String {
    Coordinates::new(ns, ew).to_string()
}

/// The destination state.
#[derive(Debug)]
pub struct Destination {
    target: Option<Rc<Location>>,
    kind: DestinationKind,
    coordinate_text: String,
    target_object_id: Option<u32>,
    target_object_position: Option<NavPosition>,
    target_indoor_position: Option<IndoorPosition>,
    target_unavailable: bool,
    saved_name: String,
    current_route: Option<Route>,
    estimated_distance: f64,
    bearing_degrees: f64,
    guidance_distance: f64,
    alternatives: Vec<Route>,
    route_origin: Option<Rc<Location>>,
    alternative_index: usize,
    alternatives_exhausted: bool,
    /// The cost profile route searches use.
    pub profile: RouteCostProfile,
}

impl Default for Destination {
    fn default() -> Self {
        Self {
            target: None,
            kind: DestinationKind::Location,
            coordinate_text: String::new(),
            target_object_id: None,
            target_object_position: None,
            target_indoor_position: None,
            target_unavailable: false,
            saved_name: String::new(),
            current_route: None,
            estimated_distance: f64::NAN,
            bearing_degrees: f64::NAN,
            guidance_distance: f64::NAN,
            alternatives: Vec::new(),
            route_origin: None,
            alternative_index: 0,
            alternatives_exhausted: false,
            profile: RouteCostProfile::ShortestWalk,
        }
    }
}

impl Destination {
    /// No destination.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The destination place.
    #[must_use]
    pub fn target(&self) -> Option<&Rc<Location>> {
        self.target.as_ref()
    }

    /// Where it came from.
    #[must_use]
    pub fn kind(&self) -> DestinationKind {
        self.kind
    }

    /// The text a coordinate destination was made from, else empty.
    #[must_use]
    pub fn coordinate_text(&self) -> &str {
        &self.coordinate_text
    }

    /// The world object of an object destination.
    #[must_use]
    pub fn target_object_id(&self) -> Option<u32> {
        self.target_object_id
    }

    /// The object's last known position.
    #[must_use]
    pub fn target_object_position(&self) -> Option<NavPosition> {
        self.target_object_position
    }

    /// The exact indoor point of a marked place.
    #[must_use]
    pub fn target_indoor_position(&self) -> Option<IndoorPosition> {
        self.target_indoor_position
    }

    /// Whether an object destination has lost its position.
    #[must_use]
    pub fn target_unavailable(&self) -> bool {
        self.target_unavailable
    }

    /// The name the settings keep for the destination (set by the setters,
    /// cleared by [`Destination::clear_destination`]).
    #[must_use]
    pub fn saved_name(&self) -> &str {
        &self.saved_name
    }

    /// The current route.
    #[must_use]
    pub fn current_route(&self) -> Option<&Route> {
        self.current_route.as_ref()
    }

    /// Whether there is a destination.
    #[must_use]
    pub fn has_destination(&self) -> bool {
        self.target.is_some()
    }

    /// The destination's name, or empty.
    #[must_use]
    pub fn target_name(&self) -> &str {
        self.target.as_ref().map_or("", |t| t.name.as_str())
    }

    /// Straight-line distance to the destination, map units, or NaN.
    #[must_use]
    pub fn estimated_distance(&self) -> f64 {
        self.estimated_distance
    }

    /// Bearing to the next waypoint, degrees (0 north, clockwise), or NaN.
    #[must_use]
    pub fn bearing_degrees(&self) -> f64 {
        self.bearing_degrees
    }

    /// Distance to the next waypoint, map units, or NaN.
    #[must_use]
    pub fn guidance_distance(&self) -> f64 {
        self.guidance_distance
    }

    /// Which alternative is shown (0 = the best route).
    #[must_use]
    pub fn alternative_index(&self) -> usize {
        self.alternative_index
    }

    /// How many alternatives are known.
    #[must_use]
    pub fn alternative_count(&self) -> usize {
        self.alternatives.len()
    }

    /// Whether Next may find more.
    #[must_use]
    pub fn alternative_has_more(&self) -> bool {
        !self.alternatives_exhausted && self.alternatives.len() < MAX_ALTERNATIVES
    }

    fn reset_alternatives(&mut self) {
        self.alternatives.clear();
        self.route_origin = None;
        self.alternative_index = 0;
        self.alternatives_exhausted = false;
    }

    /// Sets the first place with this name; false when there is none.
    pub fn set_destination_by_name(&mut self, database: &LocationDatabase, name: &str) -> bool {
        match database.find_location(name) {
            Some(location) => {
                self.set_destination(location);
                true
            }
            None => false,
        }
    }

    /// Sets a place.
    pub fn set_destination(&mut self, location: Rc<Location>) {
        self.kind = DestinationKind::Location;
        self.coordinate_text.clear();
        self.target_object_id = None;
        self.target_object_position = None;
        self.target_indoor_position = location.indoor_position;
        self.target_unavailable = false;
        self.saved_name = location.name.clone();
        self.target = Some(location);
        self.current_route = None;
        self.reset_alternatives();
    }

    /// Sets a map point; it is named by `display_text` (trimmed), or by
    /// [`coordinate_destination_name`] when that is blank.
    pub fn set_coordinate(&mut self, ns: f64, ew: f64, display_text: Option<&str>) {
        self.coordinate_text = match display_text.map(str::trim).filter(|t| !t.is_empty()) {
            Some(text) => text.to_owned(),
            None => coordinate_destination_name(ns, ew),
        };
        self.target = Some(Rc::new(Location::new(self.coordinate_text.clone(), ns, ew)));
        self.kind = DestinationKind::Coordinates;
        self.target_object_id = None;
        self.target_object_position = None;
        self.target_indoor_position = None;
        self.target_unavailable = false;
        self.saved_name = self.coordinate_text.clone();
        self.current_route = None;
        self.reset_alternatives();
    }

    /// Sets a world object; false for object 0 or one without a position.
    /// A blank name reads `Object 0x0000000C`.
    pub fn set_object(
        &mut self,
        object_id: u32,
        name: &str,
        position: Option<NavPosition>,
    ) -> bool {
        let Some(position) = position.filter(|_| object_id != 0) else {
            return false;
        };
        let name = if name.trim().is_empty() {
            format!("Object 0x{object_id:08X}")
        } else {
            name.to_owned()
        };
        self.saved_name = name.clone();
        self.target = Some(Rc::new(Location::new(
            name,
            position.north_south,
            position.east_west,
        )));
        self.kind = DestinationKind::Object;
        self.target_object_id = Some(object_id);
        self.target_object_position = Some(position);
        self.target_indoor_position = None;
        self.target_unavailable = false;
        self.coordinate_text.clear();
        self.current_route = None;
        self.reset_alternatives();
        true
    }

    /// Marks an object destination as lost.
    pub fn mark_object_unavailable(&mut self) {
        if self.kind == DestinationKind::Object {
            self.target_unavailable = true;
        }
    }

    /// Refreshes an object destination from a newer report of the same
    /// object; no position marks it lost.
    pub fn update_object(&mut self, object_id: u32, name: &str, position: Option<NavPosition>) {
        if self.kind != DestinationKind::Object || self.target_object_id != Some(object_id) {
            return;
        }
        let Some(position) = position else {
            self.mark_object_unavailable();
            return;
        };
        let name = if name.trim().is_empty() {
            self.target
                .as_ref()
                .map_or_else(|| "Object".to_owned(), |t| t.name.clone())
        } else {
            name.to_owned()
        };
        self.target = Some(Rc::new(Location::new(
            name,
            position.north_south,
            position.east_west,
        )));
        self.target_object_position = Some(position);
        self.target_unavailable = false;
        self.current_route = None;
        self.reset_alternatives();
    }

    /// Clears the destination, the route and the guidance.
    pub fn clear_destination(&mut self) {
        let profile = self.profile;
        *self = Self {
            profile,
            ..Self::default()
        };
    }

    /// Clears the route and the alternatives.
    pub fn clear_route(&mut self) {
        self.current_route = None;
        self.reset_alternatives();
    }

    /// Whether Go may keep a chosen alternative: one other than the first
    /// is shown, and the player is outdoors within 20 m of where it was
    /// planned from.
    #[must_use]
    pub fn can_keep_selected_alternative(&self, position: &NavPosition) -> bool {
        self.alternative_index > 0
            && self
                .current_route
                .as_ref()
                .is_some_and(|r| r.step_count() > 0)
            && position.is_outdoor
            && self.route_origin.as_ref().is_some_and(|origin| {
                position.coordinates().distance_to(origin.coords) * METRES_PER_MAP_UNIT
                    <= SAME_ORIGIN_METRES
            })
    }

    /// Shows the previous (-1) or next (1) alternative from `origin`,
    /// finding more when Next runs past the known ones; false when there
    /// is no such alternative.
    pub fn cycle_alternative(
        &mut self,
        direction: i32,
        origin: &Rc<Location>,
        finder: &mut RouteFinder,
    ) -> bool {
        match self.prepare_cycle(direction, origin) {
            CyclePlan::Refused => false,
            CyclePlan::Move => self.finish_cycle(direction, origin, None),
            CyclePlan::Fetch(requested) => {
                let Some(target) = self.target.clone() else {
                    return false;
                };
                let routes =
                    finder.find_route_alternatives(origin, &target, self.profile, requested);
                self.finish_cycle(direction, origin, Some((routes, requested)))
            }
        }
    }

    /// The first half of [`Destination::cycle_alternative`]: forgets the
    /// alternatives when the origin moved more than 20 m, and says whether
    /// more must be found. Find them with
    /// [`RouteFinder::alternatives_job`] for [`Destination::target`] and
    /// [`Destination::profile`], then call [`Destination::finish_cycle`].
    pub fn prepare_cycle(&mut self, direction: i32, origin: &Rc<Location>) -> CyclePlan {
        if self.target.is_none() || self.current_route.is_none() || !matches!(direction, -1 | 1) {
            return CyclePlan::Refused;
        }
        let moved = self.route_origin.as_ref().is_none_or(|planned| {
            origin.distance_to(planned) * METRES_PER_MAP_UNIT > SAME_ORIGIN_METRES
        });
        if moved {
            self.alternatives.clear();
            self.route_origin = Some(origin.clone());
            self.alternative_index = 0;
            self.alternatives_exhausted = false;
        }
        let next = self.alternative_index as i64 + i64::from(direction);
        if next >= self.alternatives.len() as i64 && self.alternative_has_more() {
            CyclePlan::Fetch((self.alternatives.len() + 1).max(2))
        } else {
            CyclePlan::Move
        }
    }

    /// The second half of [`Destination::cycle_alternative`]: `found` is
    /// the routes found for a [`CyclePlan::Fetch`] and the number asked
    /// for.
    pub fn finish_cycle(
        &mut self,
        direction: i32,
        origin: &Rc<Location>,
        found: Option<(Vec<Route>, usize)>,
    ) -> bool {
        if let Some((routes, requested)) = found {
            self.alternatives_exhausted = routes.len() < requested || requested >= MAX_ALTERNATIVES;
            self.alternatives = routes;
        }
        let next = self.alternative_index as i64 + i64::from(direction);
        if next < 0 || next >= self.alternatives.len() as i64 {
            return false;
        }
        self.alternative_index = next as usize;
        self.current_route = Some(self.alternatives[self.alternative_index].clone());
        self.update_guidance(origin);
        true
    }

    /// Plans the way out of a dungeon: the cell's dungeon is matched to
    /// places with that dungeon id, its exits are the portals named
    /// `"{dungeon} to ..."`, and the one with the shortest onward walk is
    /// taken. The route is one walk from that exit's arrival.
    pub fn try_plan_dungeon_exit(&mut self, cell_id: u32, finder: &mut RouteFinder) -> bool {
        self.try_plan_dungeon_exit_with(cell_id, finder, &[])
    }

    /// [`Destination::try_plan_dungeon_exit`], and when the places name no
    /// exit of this dungeon, the ways out the portal table knows (`known`):
    /// the one with the shortest onward route, as for the places' exits.
    pub fn try_plan_dungeon_exit_with(
        &mut self,
        cell_id: u32,
        finder: &mut RouteFinder,
        known: &[KnownExit],
    ) -> bool {
        let Some(target) = self.target.clone() else {
            return false;
        };
        if self.target_indoor_position.is_some()
            || self.target_object_position.is_some_and(|p| !p.is_outdoor)
        {
            return false;
        }
        let dungeon_id = (cell_id >> 16) as i32;
        let locations = finder.database().all_locations().to_vec();
        let mut dungeon_names: Vec<String> = Vec::new();
        for location in &locations {
            if dungeon_id != 0
                && location.dungeon_id == dungeon_id
                && !dungeon_names
                    .iter()
                    .any(|n| text::eq_ignore_case(n, &location.name))
            {
                dungeon_names.push(location.name.clone());
            }
        }
        let mut exits: Vec<&Rc<Location>> = locations
            .iter()
            .filter(|l| l.use_in_route_finding && !l.is_retired && l.has_exit_coords())
            .filter(|l| {
                dungeon_names
                    .iter()
                    .any(|name| text::starts_with_ignore_case(&l.name, &format!("{name} to ")))
            })
            .collect();
        exits.sort_by(|a, b| a.cmp_by_name(b));
        let mut selected: Option<&Rc<Location>> = None;
        let mut best = f64::INFINITY;
        for exit in exits {
            let from = Rc::new(Location::at("Dungeon exit", exit.exit_coords()));
            let onward = finder.find_route(&from, &target, self.profile);
            if onward.step_count() == 0 || onward.total_distance() >= best {
                continue;
            }
            best = onward.total_distance();
            selected = Some(exit);
        }
        let Some(selected) = selected else {
            return self.try_plan_known_dungeon_exit(&target, finder, known);
        };
        let mut route = Route::new(target.name.clone());
        let arrival = Location::at(
            format!("{} arrival ({})", selected.name, selected.id),
            selected.exit_coords(),
        );
        route.add_travel_step(Rc::new(arrival), target, "Walk after dungeon exit");
        self.current_route = Some(route);
        true
    }

    // The table's exits, chosen as the places' are: the shortest onward
    // route; the route is one walk from that exit's arrival.
    fn try_plan_known_dungeon_exit(
        &mut self,
        target: &Rc<Location>,
        finder: &mut RouteFinder,
        known: &[KnownExit],
    ) -> bool {
        let mut selected: Option<&KnownExit> = None;
        let mut best = f64::INFINITY;
        for exit in known {
            let from = Rc::new(Location::at("Dungeon exit", exit.arrival));
            let onward = finder.find_route(&from, target, self.profile);
            if onward.step_count() == 0 || onward.total_distance() >= best {
                continue;
            }
            best = onward.total_distance();
            selected = Some(exit);
        }
        let Some(selected) = selected else {
            return false;
        };
        let mut route = Route::new(target.name.clone());
        let arrival = Location::at(
            format!("{} arrival ({})", selected.name, selected.id),
            selected.arrival,
        );
        route.add_travel_step(Rc::new(arrival), target.clone(), "Walk after dungeon exit");
        self.current_route = Some(route);
        true
    }

    /// Plans the route from `current` and updates the guidance.
    pub fn calculate_route(&mut self, current: &Rc<Location>, finder: &mut RouteFinder) {
        self.reset_alternatives();
        let Some(target) = self.target.clone() else {
            self.current_route = None;
            return;
        };
        let route = finder.find_route(current, &target, self.profile);
        self.route_calculated(current, route);
    }

    /// Takes a route planned from `current` elsewhere (a
    /// [`crate::finder::RouteJob`]) as [`Destination::calculate_route`]
    /// would.
    pub fn route_calculated(&mut self, current: &Rc<Location>, route: Route) {
        self.reset_alternatives();
        if self.target.is_none() {
            self.current_route = None;
            return;
        }
        self.current_route = Some(route);
        self.route_origin = Some(current.clone());
        self.update_guidance(current);
    }

    /// Plans from the arrival of a named portal; true when the route has
    /// steps.
    pub fn try_plan_from_portal_arrival(
        &mut self,
        portal_name: &str,
        finder: &mut RouteFinder,
    ) -> bool {
        let Some(portal) = finder.database().find_location(portal_name) else {
            return false;
        };
        if self.target.is_none() || !portal.has_exit_coords() {
            return false;
        }
        let arrival = Rc::new(Location::at(
            format!("{} arrival ({})", portal.name, portal.id),
            portal.exit_coords(),
        ));
        self.calculate_route(&arrival, finder);
        self.current_route
            .as_ref()
            .is_some_and(|r| r.step_count() > 0)
    }

    /// Updates the distances and the bearing from `current`.
    pub fn update_guidance(&mut self, current: &Location) {
        let Some(target) = self.target.clone() else {
            self.estimated_distance = f64::NAN;
            self.bearing_degrees = f64::NAN;
            self.guidance_distance = f64::NAN;
            return;
        };
        self.estimated_distance = current.distance_to(&target);
        let next = self.immediate_target().unwrap_or(target);
        self.guidance_distance = current.distance_to(&next);
        self.bearing_degrees = current.coords.bearing_degrees_to(next.coords);
    }

    /// The next waypoint: the first step's end for a walk, its start for a
    /// portal or recall; the destination without a route.
    #[must_use]
    pub fn immediate_target(&self) -> Option<Rc<Location>> {
        match self.current_route.as_ref().and_then(|r| r.steps().first()) {
            Some(step) if step.kind == RouteStepKind::Travel => Some(step.to.clone()),
            Some(step) => Some(step.from.clone()),
            None => self.target.clone(),
        }
    }

    /// Drops the first step once done; with none left, the distance to the
    /// destination is 0.
    pub fn advance_step(&mut self) {
        let Some(route) = &self.current_route else {
            return;
        };
        if route.step_count() == 0 {
            return;
        }
        let mut rest = Route::new(route.destination.clone());
        for step in &route.steps()[1..] {
            rest.add_step(step.clone());
        }
        if rest.step_count() == 0 {
            self.estimated_distance = 0.0;
        }
        self.current_route = Some(rest);
    }

    /// Keeps searches off a step until the plugin is next loaded.
    pub fn avoid_step_for_session(
        &mut self,
        step: &RouteStep,
        note: &str,
        finder: &mut RouteFinder,
    ) {
        finder
            .database_mut()
            .add_session_blocked_step(BlockedStep::from_route(step, note));
    }

    /// Removes a step of the current route.
    pub fn remove_route_step(&mut self, index: usize) -> bool {
        self.current_route
            .as_mut()
            .is_some_and(|r| r.remove_step(index))
    }

    /// Moves a step of the current route.
    pub fn move_route_step(&mut self, from_index: usize, to_index: usize) -> bool {
        self.current_route
            .as_mut()
            .is_some_and(|r| r.move_step(from_index, to_index))
    }
}
