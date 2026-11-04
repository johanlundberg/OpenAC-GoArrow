//! The route in the world (new; GoArrow had none): the outdoor walks still
//! to go, from where the player stands, drawn on the ground as a thin gold
//! line, and a ring on the ground at the next waypoint. It is updated when
//! the route or its next step changes or the player has moved 10 m, at
//! most twice a second, and taken down when hidden, cleared, or indoors.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use openac_plugin::Host;
use openac_plugin::TICK_SECONDS;
use openac_plugin::ui::{
    GroundShape, LineLayer, MapPoint, MarkerAnchor, MarkerLayer, WorldLine, ground_ring,
};

use crate::coords::{Coordinates, NavPosition};
use crate::plugin::{GoArrow, Here};
use crate::route::RouteStepKind;

/// The line's colour, `0xRRGGBB`.
pub const LINE_RGB: u32 = 0xF0_B4_3C;
/// Its width, metres.
pub const LINE_WIDTH: f32 = 0.15;
/// The ring's radius, metres.
pub const RING_RADIUS: f32 = 1.5;
/// Its band, metres.
pub const RING_WIDTH: f32 = 0.2;
/// Its colour, `0xRRGGBBAA`.
pub const RING_RGBA: u32 = 0xF0_B4_3C_C8;
/// The fewest ticks between two updates (half a second).
pub const UPDATE_TICKS: u64 = (0.5 / TICK_SECONDS) as u64 + 1;
/// How far the player moves before the line is drawn again from them,
/// metres.
pub const MOVE_METRES: f64 = 10.0;

/// A number that changes when the route, its step or the destination does.
#[must_use]
pub fn route_signature(app: &GoArrow) -> u64 {
    let mut hasher = DefaultHasher::new();
    let destination = app.destination();
    destination.target_name().hash(&mut hasher);
    destination.alternative_index().hash(&mut hasher);
    if let Some(route) = destination.current_route() {
        route.step_count().hash(&mut hasher);
        for step in route.steps() {
            (step.kind as u8).hash(&mut hasher);
            step.to.name.hash(&mut hasher);
            step.from.coords.ns.to_bits().hash(&mut hasher);
            step.from.coords.ew.to_bits().hash(&mut hasher);
            step.to.coords.ns.to_bits().hash(&mut hasher);
            step.to.coords.ew.to_bits().hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn point(c: Coordinates, elevation: f64) -> MapPoint {
    MapPoint {
        east_west: c.ew,
        north_south: c.ns,
        elevation,
    }
}

/// The walks still to go as world lines: the first from the player, then
/// every walk of the route (those after a portal start where it lets out).
#[must_use]
pub fn route_lines(app: &GoArrow, position: &NavPosition) -> Vec<WorldLine> {
    let Some(route) = app.destination().current_route() else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for (i, step) in route.steps().iter().enumerate() {
        if step.kind != RouteStepKind::Travel {
            continue;
        }
        let from = if i == 0 {
            position.coordinates()
        } else {
            step.from.coords
        };
        // A walk inside a hub is between its made-up points, nowhere on
        // the ground.
        if from.is_none()
            || step.to.coords.is_none()
            || route.hub_at(from).is_some()
            || route.hub_at(step.to.coords).is_some()
        {
            continue;
        }
        lines.push(WorldLine {
            start: point(from, position.elevation),
            end: point(step.to.coords, position.elevation),
            color_rgb: LINE_RGB,
            follow_terrain: true,
            width_metres: LINE_WIDTH,
        });
    }
    lines
}

/// The ring at the next waypoint.
#[must_use]
pub fn next_ring(app: &GoArrow, position: &NavPosition) -> Option<GroundShape> {
    let next = app.destination().immediate_target()?;
    if !next.has_coordinates() {
        return None;
    }
    Some(ground_ring(
        MarkerAnchor::Position(point(next.coords, position.elevation), true),
        RING_RADIUS,
        RING_WIDTH,
        RING_RGBA,
    ))
}

/// The route on the ground.
#[derive(Debug, Default)]
pub struct Ground {
    lines: Option<LineLayer>,
    markers: Option<MarkerLayer>,
    shown: Option<(u64, NavPosition)>,
    last_update: Option<u64>,
    ticks: u64,
    drawn: usize,
}

impl Ground {
    /// How many lines are up.
    #[must_use]
    pub fn drawn(&self) -> usize {
        self.drawn
    }

    /// Whether anything is up.
    #[must_use]
    pub fn is_shown(&self) -> bool {
        self.shown.is_some()
    }

    /// Takes everything down.
    pub fn clear(&mut self, host: &mut Host<'_>) {
        if self.shown.take().is_some() {
            if let Some(lines) = self.lines {
                lines.set_lines(host, &[]);
            }
            if let Some(markers) = self.markers {
                markers.set_shapes(host, &[]);
            }
        }
        self.drawn = 0;
    }

    /// One tick: draws, moves or takes down the route.
    pub fn tick(&mut self, app: &mut GoArrow, host: &mut Host<'_>, here: &Here) {
        self.ticks += 1;
        let wanted = app.settings().route_line_visible
            && here.outdoors()
            && app
                .destination()
                .current_route()
                .is_some_and(|r| r.step_count() > 0);
        if !wanted {
            self.clear(host);
            return;
        }
        if self
            .last_update
            .is_some_and(|at| self.ticks - at < UPDATE_TICKS)
        {
            return;
        }
        let signature = route_signature(app);
        if let Some((shown, at)) = self.shown
            && shown == signature
            && here.position.horizontal_distance_metres(&at) < MOVE_METRES
        {
            return;
        }
        let lines = route_lines(app, &here.position);
        let ring: Vec<GroundShape> = next_ring(app, &here.position).into_iter().collect();
        let line_layer = *self.lines.get_or_insert_with(|| host.create_line_layer());
        let marker_layer = *self
            .markers
            .get_or_insert_with(|| host.create_marker_layer());
        line_layer.set_lines(host, &lines);
        marker_layer.set_shapes(host, &ring);
        self.drawn = lines.len();
        self.shown = Some((signature, here.position));
        self.last_update = Some(self.ticks);
    }
}
