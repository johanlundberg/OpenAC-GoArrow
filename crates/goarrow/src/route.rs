//! A route (`RouteFinding/Route.cs`): the steps from an origin to a
//! destination, each a walk, a portal or a recall, with the totals the
//! route window and `/go status` show.

use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use crate::coords::Coordinates;
use crate::distance::format_travel_distance;
use crate::location::{Location, LocationType};
use crate::text;

/// What a step does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteStepKind {
    /// Walk (or "Arrived", a walk of nothing).
    Travel,
    /// Use a portal.
    Portal,
    /// Cast a recall or use a lifestone.
    Recall,
}

impl RouteStepKind {
    /// The kind's name, as corrections files store it (`Travel`, `Portal`,
    /// `Recall`).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Travel => "Travel",
            Self::Portal => "Portal",
            Self::Recall => "Recall",
        }
    }
}

/// One step of a route.
#[derive(Debug, Clone)]
pub struct RouteStep {
    /// What the step does.
    pub kind: RouteStepKind,
    /// Where it starts: the exact origin for a route's first step.
    pub from: Rc<Location>,
    /// The graph location it leaves from; differs from `from` when the
    /// route begins at the exact origin.
    pub graph_from: Rc<Location>,
    /// Where it ends.
    pub to: Rc<Location>,
    /// The walk's length, map units (0 for portals and recalls).
    pub distance: f64,
    /// How: `Walk`, `Walk to start`, `Arrived`, the portal's or recall's
    /// name.
    pub via: String,
    /// The world object to use, when known (the planner leaves it 0).
    pub object_id: u32,
    /// The object's capability bits (the planner leaves them 0).
    pub object_capabilities: u32,
    /// How long an interaction may take.
    pub interaction_timeout: Duration,
    /// How many times an interaction is tried again.
    pub max_retries: i32,
}

impl RouteStep {
    /// A step; `graph_from` is `from`.
    #[must_use]
    pub fn new(
        kind: RouteStepKind,
        from: Rc<Location>,
        to: Rc<Location>,
        distance: f64,
        via: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            graph_from: from.clone(),
            from,
            to,
            distance,
            via: via.into(),
            object_id: 0,
            object_capabilities: 0,
            interaction_timeout: Duration::from_secs(15),
            max_retries: 1,
        }
    }

    fn via_or_destination(&self) -> &str {
        if self.via.trim().is_empty() {
            &self.to.name
        } else {
            &self.via
        }
    }
}

/// `Arrived: To`, `Walk: To (31 m)`, `Portal: Via` or `Recall: Via` (the
/// destination's name when the via is blank).
impl fmt::Display for RouteStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            RouteStepKind::Travel if text::eq_ignore_case(&self.via, "Arrived") => {
                write!(f, "Arrived: {}", self.to.name)
            }
            RouteStepKind::Travel => {
                write!(
                    f,
                    "Walk: {} ({})",
                    self.to.name,
                    format_travel_distance(self.distance)
                )
            }
            RouteStepKind::Portal => write!(f, "Portal: {}", self.via_or_destination()),
            RouteStepKind::Recall => write!(f, "Recall: {}", self.via_or_destination()),
        }
    }
}

/// The indoor hubs the Atlas lists every portal inside of at one made-up
/// point in the sea south of Dereth (the Town Network's 99.1S 8.7E): a
/// portal name beginning so, and the hub's name.
pub const HUBS: [(&str, &str); 2] = [
    ("Town Network (", "the Town Network"),
    ("Facility Hub ", "the Facility Hub"),
];

/// How near a hub's made-up point a point counts as it, map units.
const HUB_REACH: f64 = 0.5;

/// The hub a point of a route is the made-up point of: the point one of
/// the route's portals inside a hub ([`HUBS`]) is listed at.
#[must_use]
pub fn hub_at(steps: &[RouteStep], at: Coordinates) -> Option<&'static str> {
    if at.is_none() {
        return None;
    }
    steps
        .iter()
        .flat_map(|step| [&step.from, &step.to])
        // A portal's arrival location carries its name to where it lets
        // out, but is no portal.
        .filter(|location| location.kind.intersects(LocationType::ANY_PORTAL))
        .find_map(|location| {
            let (_, hub) = HUBS
                .iter()
                .find(|(prefix, _)| text::starts_with_ignore_case(&location.name, prefix))?;
            (location.has_coordinates() && location.coords.distance_to(at) <= HUB_REACH)
                .then_some(*hub)
        })
}

/// A route: its steps in order and their totals.
#[derive(Debug, Clone, Default)]
pub struct Route {
    /// The destination's name.
    pub destination: String,
    /// Why the route is what it is, when it is not a graph route
    /// (`Direct walk (no graph route found)`), else empty.
    pub description: String,
    steps: Vec<RouteStep>,
    total_distance: f64,
    portal_count: usize,
}

impl Route {
    /// An empty route to a destination.
    #[must_use]
    pub fn new(destination: impl Into<String>) -> Self {
        Self {
            destination: destination.into(),
            ..Self::default()
        }
    }

    /// The steps.
    #[must_use]
    pub fn steps(&self) -> &[RouteStep] {
        &self.steps
    }

    /// The number of steps.
    #[must_use]
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// The walked distance (travel steps only), map units.
    #[must_use]
    pub fn total_distance(&self) -> f64 {
        self.total_distance
    }

    /// The hub a point of this route is the made-up point of ([`hub_at`]).
    #[must_use]
    pub fn hub_at(&self, at: Coordinates) -> Option<&'static str> {
        hub_at(&self.steps, at)
    }

    /// The number of portal steps.
    #[must_use]
    pub fn portal_count(&self) -> usize {
        self.portal_count
    }

    fn count(&mut self, step: &RouteStep) {
        match step.kind {
            RouteStepKind::Travel => self.total_distance += step.distance,
            RouteStepKind::Portal => self.portal_count += 1,
            RouteStepKind::Recall => {}
        }
    }

    /// Adds a step at the end.
    pub fn add_step(&mut self, step: RouteStep) {
        self.count(&step);
        self.steps.push(step);
    }

    /// Adds a step at the start.
    pub fn prepend_step(&mut self, step: RouteStep) {
        self.count(&step);
        self.steps.insert(0, step);
    }

    /// Makes the first step start at the exact origin, keeping the graph
    /// location it leaves from.
    pub fn set_first_step_origin(&mut self, origin: Rc<Location>) {
        if let Some(first) = self.steps.first_mut() {
            first.from = origin;
        }
    }

    /// Adds a walk; its distance is the straight line between the two.
    pub fn add_travel_step(&mut self, from: Rc<Location>, to: Rc<Location>, via: &str) {
        let distance = from.coords.distance_to(to.coords);
        self.add_step(RouteStep::new(
            RouteStepKind::Travel,
            from,
            to,
            distance,
            via,
        ));
    }

    /// Adds a portal.
    pub fn add_portal_step(&mut self, from: Rc<Location>, to: Rc<Location>, portal_name: &str) {
        self.add_step(RouteStep::new(
            RouteStepKind::Portal,
            from,
            to,
            0.0,
            portal_name,
        ));
    }

    /// Adds a recall.
    pub fn add_recall_step(&mut self, from: Rc<Location>, to: Rc<Location>, recall_name: &str) {
        self.add_step(RouteStep::new(
            RouteStepKind::Recall,
            from,
            to,
            0.0,
            recall_name,
        ));
    }

    /// Removes a step and recounts the totals; false when out of range.
    pub fn remove_step(&mut self, index: usize) -> bool {
        if index >= self.steps.len() {
            return false;
        }
        self.steps.remove(index);
        self.total_distance = self
            .steps
            .iter()
            .filter(|s| s.kind == RouteStepKind::Travel)
            .map(|s| s.distance)
            .sum();
        self.portal_count = self
            .steps
            .iter()
            .filter(|s| s.kind == RouteStepKind::Portal)
            .count();
        true
    }

    /// Moves a step to another place; false when either is out of range.
    pub fn move_step(&mut self, from_index: usize, to_index: usize) -> bool {
        if from_index >= self.steps.len() || to_index >= self.steps.len() {
            return false;
        }
        let step = self.steps.remove(from_index);
        self.steps.insert(to_index, step);
        true
    }

    /// Removes every step.
    pub fn clear(&mut self) {
        self.steps.clear();
        self.total_distance = 0.0;
        self.portal_count = 0;
    }
}

/// `Destination: 3 steps, 1.2 km, 1 portals`.
impl fmt::Display for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} steps, {}, {} portals",
            self.destination,
            self.steps.len(),
            format_travel_distance(self.total_distance),
            self.portal_count
        )
    }
}
