//! Route corrections (`RouteFinding/AtlasCorrections.cs`): Atlas portal
//! arrivals that are wrong in the Atlas data, by Atlas id, and route steps
//! that do not work in the game. A wrong arrival makes the route walk on
//! from a place the portal never reaches, so navigation re-plans the same
//! portal after every arrival; a blocked step is never used again.
//!
//! The file is JSON (`GoArrow/atlas-corrections.json`, first written from
//! [`SHIPPED`]). Member names match in any case; `//` and `/* */` comments
//! and trailing commas are allowed, as the C# reader allowed them. It is
//! written indented with the C# member names.

use std::collections::HashMap;
use std::fmt;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::coords::Coordinates;
use crate::graph::RouteEdgeKind;
use crate::location::Location;
use crate::route::RouteStep;
use crate::text;

/// The corrections shipped with the plugin (OpenAC-GoArrow's
/// `EmbeddedResources/AtlasCorrections.json`).
pub const SHIPPED: &str = include_str!("../data/atlas-corrections.json");

/// An arrival that replaces the Atlas value.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ArrivalCorrection {
    /// The Atlas location id; the only field used for matching.
    pub id: i32,
    /// The Atlas name, for people reading the file.
    pub name: String,
    /// The right arrival in map notation (`52.2S, 82.5E`).
    pub arrival: String,
    /// Why.
    pub note: String,
}

/// A route step that does not work in the game. Each end matches a graph
/// location by Atlas id when the id is positive, otherwise by name
/// (ignoring case). A blocked walk is blocked both ways.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct BlockedStep {
    /// `Travel`, `Portal` or `Recall`, as in the route's steps.
    pub kind: String,
    /// The name of the location the step leaves from.
    pub from: String,
    /// Its Atlas id, or 0.
    pub from_id: i32,
    /// The name of the location the step reaches.
    pub to: String,
    /// Its Atlas id, or 0.
    pub to_id: i32,
    /// Why.
    pub note: String,
}

impl BlockedStep {
    /// The block for a route step: its graph start and its end.
    #[must_use]
    pub fn from_route(step: &RouteStep, note: &str) -> Self {
        Self {
            kind: step.kind.name().to_owned(),
            from: step.graph_from.name.clone(),
            from_id: step.graph_from.id,
            to: step.to.name.clone(),
            to_id: step.to.id,
            note: note.to_owned(),
        }
    }

    /// Whether this blocks the graph edge `from` → `to` of `kind`.
    #[must_use]
    pub fn blocks(&self, from: &Location, to: &Location, kind: RouteEdgeKind) -> bool {
        let wanted = match kind {
            RouteEdgeKind::Walk => "Travel",
            RouteEdgeKind::Portal => "Portal",
            RouteEdgeKind::Recall | RouteEdgeKind::Lifestone => "Recall",
        };
        if !text::eq_ignore_case(&self.kind, wanted) {
            return false;
        }
        if matches(from, &self.from, self.from_id) && matches(to, &self.to, self.to_id) {
            return true;
        }
        kind == RouteEdgeKind::Walk
            && matches(to, &self.from, self.from_id)
            && matches(from, &self.to, self.to_id)
    }

    /// Whether `other` blocks the same edges: the same kind, ends and ids
    /// (names ignoring case; a name does not count where the id is used).
    fn same_edges(&self, other: &Self) -> bool {
        let end = |name: &str, id: i32, other_name: &str, other_id: i32| {
            id == other_id && (id > 0 || text::eq_ignore_case(name, other_name))
        };
        text::eq_ignore_case(&self.kind, &other.kind)
            && end(&self.from, self.from_id, &other.from, other.from_id)
            && end(&self.to, self.to_id, &other.to, other.to_id)
    }
}

fn matches(location: &Location, name: &str, id: i32) -> bool {
    if id > 0 {
        location.id == id
    } else {
        text::eq_ignore_case(&location.name, name)
    }
}

/// A corrections file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct AtlasCorrections {
    /// The file version (1).
    pub version: i32,
    /// Arrival corrections.
    pub arrivals: Vec<ArrivalCorrection>,
    /// Blocked steps.
    pub blocked_steps: Vec<BlockedStep>,
}

impl Default for AtlasCorrections {
    fn default() -> Self {
        Self {
            version: 1,
            arrivals: Vec::new(),
            blocked_steps: Vec::new(),
        }
    }
}

/// Why a corrections file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrectionsError(pub String);

impl fmt::Display for CorrectionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CorrectionsError {}

fn error(message: impl Into<String>) -> CorrectionsError {
    CorrectionsError(message.into())
}

impl AtlasCorrections {
    /// Reads a corrections file. `null` is an empty file.
    pub fn parse(json: &str) -> Result<Self, CorrectionsError> {
        let cleaned = strip_comments_and_trailing_commas(json)?;
        let value: Value = serde_json::from_str(&cleaned).map_err(|e| error(e.to_string()))?;
        let mut corrections = Self::default();
        let root = match value {
            Value::Null => return Ok(corrections),
            Value::Object(root) => root,
            _ => return Err(error("The corrections file is not a JSON object.")),
        };
        for (key, value) in &root {
            match key.to_ascii_lowercase().as_str() {
                "version" => corrections.version = int(value, "Version")?,
                "arrivals" => {
                    corrections.arrivals = objects(value, "Arrivals")?
                        .map(|entry| {
                            let mut arrival = ArrivalCorrection::default();
                            for (key, value) in entry? {
                                match key.to_ascii_lowercase().as_str() {
                                    "id" => arrival.id = int(value, "Id")?,
                                    "name" => arrival.name = string(value, "Name")?,
                                    "arrival" => arrival.arrival = string(value, "Arrival")?,
                                    "note" => arrival.note = string(value, "Note")?,
                                    _ => {}
                                }
                            }
                            Ok(arrival)
                        })
                        .collect::<Result<_, CorrectionsError>>()?;
                }
                "blockedsteps" => {
                    corrections.blocked_steps = objects(value, "BlockedSteps")?
                        .map(|entry| {
                            let mut step = BlockedStep::default();
                            for (key, value) in entry? {
                                match key.to_ascii_lowercase().as_str() {
                                    "kind" => step.kind = string(value, "Kind")?,
                                    "from" => step.from = string(value, "From")?,
                                    "fromid" => step.from_id = int(value, "FromId")?,
                                    "to" => step.to = string(value, "To")?,
                                    "toid" => step.to_id = int(value, "ToId")?,
                                    "note" => step.note = string(value, "Note")?,
                                    _ => {}
                                }
                            }
                            Ok(step)
                        })
                        .collect::<Result<_, CorrectionsError>>()?;
                }
                _ => {}
            }
        }
        Ok(corrections)
    }

    /// The file text: indented JSON with the C# member names.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// The sources as one set, as route searches use them: an arrival
    /// replaces an earlier source's arrival with the same Atlas id (in its
    /// place); a blocked step that blocks the same edges as an earlier one
    /// (the same kind, ends and ids, ignoring case and the note) replaces
    /// it. The player's file starts as a copy of the shipped one, so this
    /// lists each correction once.
    #[must_use]
    pub fn merged(sources: &[Self]) -> Self {
        let mut merged = Self::default();
        for source in sources {
            for arrival in &source.arrivals {
                let same = merged
                    .arrivals
                    .iter()
                    .position(|a| a == arrival || (a.id > 0 && a.id == arrival.id));
                match same {
                    Some(earlier) => merged.arrivals[earlier].clone_from(arrival),
                    None => merged.arrivals.push(arrival.clone()),
                }
            }
            for step in &source.blocked_steps {
                match merged.blocked_steps.iter_mut().find(|s| s.same_edges(step)) {
                    Some(earlier) => earlier.clone_from(step),
                    None => merged.blocked_steps.push(step.clone()),
                }
            }
        }
        merged
    }

    /// Arrival coordinates by Atlas id; later sources override earlier
    /// ones. An entry without an id or with an arrival that does not read
    /// is reported in `errors` and not applied.
    #[must_use]
    pub fn arrivals_by_id(
        sources: &[&Self],
        errors: &mut Vec<String>,
    ) -> HashMap<i32, Coordinates> {
        let mut result = HashMap::new();
        for source in sources {
            for entry in &source.arrivals {
                if entry.id <= 0 {
                    errors.push(format!(
                        "Arrival correction '{}' has no Atlas id.",
                        entry.name
                    ));
                } else if let Some(arrival) = Coordinates::parse(&entry.arrival) {
                    result.insert(entry.id, arrival);
                } else {
                    errors.push(format!(
                        "Arrival correction {} has unreadable arrival '{}'.",
                        entry.id, entry.arrival
                    ));
                }
            }
        }
        result
    }
}

fn int(value: &Value, name: &str) -> Result<i32, CorrectionsError> {
    value
        .as_i64()
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| error(format!("{name} must be a whole number.")))
}

fn string(value: &Value, name: &str) -> Result<String, CorrectionsError> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Null => Ok(String::new()),
        _ => Err(error(format!("{name} must be text."))),
    }
}

fn objects<'a>(
    value: &'a Value,
    name: &'a str,
) -> Result<
    impl Iterator<Item = Result<&'a Map<String, Value>, CorrectionsError>> + 'a,
    CorrectionsError,
> {
    let Value::Array(items) = value else {
        return Err(error(format!("{name} must be a list.")));
    };
    Ok(items.iter().map(move |item| {
        item.as_object()
            .ok_or_else(|| error(format!("Each entry of {name} must be an object.")))
    }))
}

/// Removes `//` and `/* */` comments and the commas before a `]` or `}`,
/// outside strings.
fn strip_comments_and_trailing_commas(json: &str) -> Result<String, CorrectionsError> {
    let mut out = String::with_capacity(json.len());
    let mut chars = json.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push(c);
                while let Some(c) = chars.next() {
                    out.push(c);
                    if c == '\\' {
                        if let Some(escaped) = chars.next() {
                            out.push(escaped);
                        }
                    } else if c == '"' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                let mut closed = false;
                for c in chars.by_ref() {
                    if previous == '*' && c == '/' {
                        closed = true;
                        break;
                    }
                    previous = c;
                }
                if !closed {
                    return Err(error("A comment is not closed."));
                }
                out.push(' ');
            }
            ']' | '}' => {
                let kept = out.trim_end().len();
                if out[..kept].ends_with(',') {
                    out.truncate(kept - 1);
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    Ok(out)
}
