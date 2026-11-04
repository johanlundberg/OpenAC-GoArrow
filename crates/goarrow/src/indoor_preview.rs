//! The planned indoor path the window lists (`GoArrowIndoorPathPreview.cs`):
//! while the destination is an indoor target in this dungeon, the client
//! is asked for a path preview (nothing walks), again only once the
//! character has moved on (another cell, 10 m, or 2 m of height) and at
//! most every 3 seconds. A plan for an earlier target or from an origin
//! left behind is ignored.

use openac_plugin::Host;
use openac_plugin::navigation::{NavigationPlan, NavigationPlanStatus};
use openac_plugin::world::{METRES_PER_MAP_UNIT, Position};

use crate::navigator::IndoorPreview;
use crate::text::format_fixed;

/// The most waypoints listed.
pub const MAX_DISPLAYED_WAYPOINTS: usize = 200;
/// The least time between two previews, seconds.
pub const MIN_REQUEST_INTERVAL_SECONDS: f64 = 3.0;

/// Where a preview leads: an object (`object_id` not 0) or a place.
#[derive(Debug, Clone, PartialEq)]
struct PreviewTarget {
    player_id: u32,
    object_id: u32,
    position: Position,
}

fn same_height(first: &Position, second: &Position) -> bool {
    first.elevation == second.elevation
        || (first.elevation - second.elevation).abs() * METRES_PER_MAP_UNIT < 2.0
}

fn same_target(first: &PreviewTarget, second: &PreviewTarget) -> bool {
    first.player_id == second.player_id
        && first.object_id == second.object_id
        && first.position.cell_id == second.position.cell_id
        && first.position.horizontal_distance_metres(&second.position) < 2.0
        && same_height(&first.position, &second.position)
}

fn same_origin(first: &Position, second: &Position) -> bool {
    first.cell_id == second.cell_id
        && first.horizontal_distance_metres(second) < 10.0
        && same_height(first, second)
}

/// The preview's state.
#[derive(Debug, Default)]
pub struct IndoorPathPreview {
    target: Option<PreviewTarget>,
    request_origin: Position,
    result_origin: Position,
    pending: Option<u32>,
    answer: Option<NavigationPlan>,
    last_request_at: Option<f64>,
    has_result: bool,
    view: Option<IndoorPreview>,
}

impl IndoorPathPreview {
    /// Nothing shown.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What the window shows, while a preview is shown.
    #[must_use]
    pub fn view(&self) -> Option<&IndoorPreview> {
        self.view.as_ref()
    }

    /// The handle of the preview waited for.
    #[must_use]
    pub fn pending(&self) -> Option<u32> {
        self.pending
    }

    /// Hides the preview and forgets its target.
    pub fn clear(&mut self) {
        self.target = None;
        self.pending = None;
        self.answer = None;
        self.has_result = false;
        self.last_request_at = None;
        self.view = None;
    }

    /// A preview's answer; false when it is not the one waited for.
    pub fn plan_arrived(&mut self, plan: &NavigationPlan) -> bool {
        if self.pending == Some(plan.handle) {
            self.answer = Some(plan.clone());
            true
        } else {
            false
        }
    }

    /// One tick: `target` is the object (not 0) or place the preview
    /// leads to, or none to hide it.
    pub fn tick(
        &mut self,
        host: &mut Host<'_>,
        target: Option<(u32, Position)>,
        arrival_meters: f32,
        now: f64,
    ) {
        let Some((object_id, position)) = target else {
            self.clear();
            return;
        };
        let snapshot = host.navigation();
        let target = PreviewTarget {
            player_id: snapshot.local_object_id,
            object_id,
            position: position.clone(),
        };
        if self
            .target
            .as_ref()
            .is_none_or(|previous| !same_target(previous, &target))
        {
            let same_moving_object = self.target.as_ref().is_some_and(|old| {
                old.object_id > 0
                    && old.player_id == target.player_id
                    && old.object_id == target.object_id
            });
            self.target = Some(target);
            self.pending = None;
            self.answer = None;
            self.has_result = false;
            if !same_moving_object {
                self.last_request_at = None;
            }
            self.show_planning();
        }
        if self.view.is_none() {
            self.show_planning();
        }

        if self.pending.is_some()
            && let Some(plan) = self.answer.take()
        {
            self.pending = None;
            if same_origin(&snapshot.position, &self.request_origin) {
                self.set_result(&plan);
                self.result_origin = self.request_origin.clone();
                self.has_result = true;
            }
        }
        if self.pending.is_some() {
            return;
        }
        if self.has_result && same_origin(&snapshot.position, &self.result_origin) {
            return;
        }
        if self
            .last_request_at
            .is_some_and(|at| now - at < MIN_REQUEST_INTERVAL_SECONDS)
        {
            return;
        }
        self.request_origin = snapshot.position.clone();
        self.last_request_at = Some(now);
        self.has_result = false;
        self.show_planning();
        self.pending = Some(if object_id != 0 {
            host.preview_path(object_id, arrival_meters)
        } else {
            host.preview_path_to_place(position, arrival_meters)
        });
    }

    fn show_planning(&mut self) {
        self.view = Some(IndoorPreview {
            summary: "Planning indoor path...".to_owned(),
            ..IndoorPreview::default()
        });
    }

    fn set_result(&mut self, plan: &NavigationPlan) {
        if plan.status != NavigationPlanStatus::Routed {
            let mut summary = if plan.status == NavigationPlanStatus::NoRoute {
                "No indoor path found.".to_owned()
            } else {
                format!("Indoor path unavailable ({:?}).", plan.status)
            };
            if !plan.reason.trim().is_empty() {
                summary.push(' ');
                summary.push_str(&plan.reason);
            }
            self.view = Some(IndoorPreview {
                summary,
                ..IndoorPreview::default()
            });
            return;
        }
        let count = plan.path.len();
        let summary = format!(
            "Indoor path: {} m, {count} waypoints",
            format_fixed(f64::from(plan.length_meters), 0, 1)
        );
        let mut waypoints: Vec<String> = plan
            .path
            .iter()
            .take(MAX_DISPLAYED_WAYPOINTS)
            .enumerate()
            .map(|(index, point)| {
                let local = point.landblock_local();
                format!(
                    "{}. Cell 0x{:08X}: E {}, N {}, Z {} m",
                    index + 1,
                    point.cell_id,
                    format_fixed(f64::from(local[0]), 0, 1),
                    format_fixed(f64::from(local[1]), 0, 1),
                    format_fixed(f64::from(local[2]), 0, 1)
                )
            })
            .collect();
        if count > MAX_DISPLAYED_WAYPOINTS {
            waypoints.push(format!(
                "... {} more waypoints",
                count - MAX_DISPLAYED_WAYPOINTS
            ));
        }
        let points = plan
            .path
            .iter()
            .map(|p| (p.cell_id, p.landblock_local()))
            .collect();
        self.view = Some(IndoorPreview {
            summary,
            waypoints,
            points,
        });
    }
}
