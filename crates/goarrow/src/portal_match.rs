//! How a live portal is matched to the place a route names
//! (`GoArrowNavigator.cs`'s name rules): the destination a portal's name
//! or appraisal label leads to, a looser form of a name, and the tests the
//! navigator applies to world objects. All of it is pure text and numbers.

use openac_plugin::world::{ObjectClass, WorldObject, capabilities};

use crate::coords::Coordinates;
use crate::route::{RouteStep, RouteStepKind};
use crate::text;

/// The destination a portal name leads to: `Town Network Portal to `,
/// `Portal to `, `Portal for ` and `Gateway to ` are dropped from the
/// front, a `Town Network ... to ` or `Facility Hub ... to ` prefix up to
/// its last ` to `, and ` Town Network Portal` or ` Portal` from the end
/// (`Portal to Sawato` and `Sawato Portal` both lead to `Sawato`).
#[must_use]
pub fn portal_destination_name(name: &str) -> String {
    let mut value = name.trim();
    let prefixes = [
        "Town Network Portal to ",
        "Portal to ",
        "Portal for ",
        "Gateway to ",
    ];
    if let Some(prefix) = prefixes
        .iter()
        .find(|p| text::starts_with_ignore_case(value, p))
    {
        value = &value[prefix_len(value, prefix)..];
    } else if (text::starts_with_ignore_case(value, "Town Network ")
        || text::starts_with_ignore_case(value, "Facility Hub "))
        && let Some(to) = text::rfind_ignore_case(value, " to ")
    {
        value = &value[to + " to ".len()..];
    }
    if text::ends_with_ignore_case(value, " Town Network Portal") {
        value = &value[..suffix_start(value, " Town Network Portal")];
    } else if text::ends_with_ignore_case(value, " Portal") {
        value = &value[..suffix_start(value, " Portal")];
    }
    value.trim().to_owned()
}

/// The byte length of the part of `value` a case-insensitive `prefix`
/// matched.
fn prefix_len(value: &str, prefix: &str) -> usize {
    value
        .char_indices()
        .nth(prefix.chars().count())
        .map_or(value.len(), |(i, _)| i)
}

/// Where the part of `value` a case-insensitive `suffix` matched starts.
fn suffix_start(value: &str, suffix: &str) -> usize {
    value
        .char_indices()
        .rev()
        .nth(suffix.chars().count() - 1)
        .map_or(0, |(i, _)| i)
}

/// The destination an appraisal label names: a trailing `(coordinates)`
/// and trailing full stops dropped, then read as
/// [`portal_destination_name`] (`Folthid Estate (8.8S, 53.5E).` names
/// `Folthid Estate`).
#[must_use]
pub fn portal_destination_label(label: &str) -> String {
    let mut label = label;
    if let Some(open) = label.rfind('(')
        && Coordinates::parse(&label[open..]).is_some()
    {
        label = &label[..open];
    }
    portal_destination_name(label.trim().trim_end_matches('.'))
}

/// A name without a leading `the ` or a trailing ` (...)`: Atlas names
/// tell same-named places apart (`Halls of Metos (Imuth Maer Doquin)`)
/// where the live portal says `Gateway to the Halls of Metos`.
#[must_use]
pub fn loose_name(name: &str) -> String {
    let mut value = name.trim();
    if text::starts_with_ignore_case(value, "the ") {
        value = &value[prefix_len(value, "the ")..];
    }
    if value.ends_with(')')
        && let Some(open) = value.rfind(" (")
        && open > 0
    {
        value = &value[..open];
    }
    value.trim().to_owned()
}

/// Whether a portal leads to `destination` (already a destination name):
/// by its own name, by its appraisal label, or by the loose forms of its
/// name and the destination. Callers bound the distance.
#[must_use]
pub fn portal_matches_destination(portal: &WorldObject, destination: &str) -> bool {
    let live = portal_destination_name(&portal.name);
    text::eq_ignore_case(&live, destination)
        || portal.portal_destination.as_deref().is_some_and(|known| {
            text::eq_ignore_case(&portal_destination_label(known), destination)
        })
        || text::eq_ignore_case(&loose_name(&live), &loose_name(destination))
}

/// Whether a text is missing or only white space.
#[must_use]
pub fn is_blank(value: Option<&str>) -> bool {
    value.is_none_or(|v| v.trim().is_empty())
}

/// A `Surface Portal` whose destination is not known yet: it must be
/// identified before it is used.
#[must_use]
pub fn is_unknown_surface_portal(portal: &WorldObject) -> bool {
    text::eq_ignore_case(&portal.name, "Surface Portal")
        && is_blank(portal.portal_destination.as_deref())
}

/// Classified as a portal, or with the portal capability.
#[must_use]
pub fn is_portal(object: &WorldObject) -> bool {
    object.object_class == ObjectClass::Portal || has_portal_capability(object)
}

/// With the portal capability.
#[must_use]
pub fn has_portal_capability(object: &WorldObject) -> bool {
    object.capabilities & capabilities::PORTAL != 0
}

/// Whether two cells' landblocks lie within two landblocks of each other
/// both ways.
#[must_use]
pub fn are_nearby_indoor_landblocks(first: u32, second: u32) -> bool {
    let (first_x, first_y) = ((first >> 24) as i32, ((first >> 16) & 0xFF) as i32);
    let (second_x, second_y) = ((second >> 24) as i32, ((second >> 16) & 0xFF) as i32);
    (first_x - second_x).abs() <= 2 && (first_y - second_y).abs() <= 2
}

/// The exit a walk from a dungeon arrival leads out by: the text after the
/// last ` to ` in `"{dungeon} to {exit} arrival (id)"`, read as a
/// destination name. None for other steps.
#[must_use]
pub fn route_dungeon_exit_name(step: &RouteStep) -> Option<String> {
    if step.kind != RouteStepKind::Travel {
        return None;
    }
    let origin = step.from.name.as_str();
    let arrival = text::rfind_ignore_case(origin, " arrival (")?;
    let origin = &origin[..arrival];
    let destination = text::rfind_ignore_case(origin, " to ")?;
    let name = portal_destination_name(&origin[destination + " to ".len()..]);
    (!name.is_empty()).then_some(name)
}

/// The names of an object's capability bits, as the C# flags read
/// (`Interactable, Portal`, or `None`).
#[must_use]
pub fn capability_names(bits: u32) -> String {
    const NAMES: [(u32, &str); 7] = [
        (capabilities::INTERACTABLE, "Interactable"),
        (capabilities::PORTAL, "Portal"),
        (capabilities::DOOR, "Door"),
        (capabilities::VENDOR, "Vendor"),
        (capabilities::CONTAINER, "Container"),
        (capabilities::PLAYER, "Player"),
        (capabilities::NPC, "Npc"),
    ];
    let names: Vec<&str> = NAMES
        .iter()
        .filter(|(bit, _)| bits & bit != 0)
        .map(|(_, name)| *name)
        .collect();
    if names.is_empty() {
        "None".to_owned()
    } else {
        names.join(", ")
    }
}
