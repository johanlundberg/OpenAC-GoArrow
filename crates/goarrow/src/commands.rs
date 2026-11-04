//! The `/go` commands (`GoArrowCommands.cs`, `GoArrowCommandDefinition.cs`):
//! the arguments as OpenAC's command line parser splits them, every
//! subcommand with GoArrow's chat lines, the help, and completion.

use openac_plugin::navigation::GoToState;
use openac_plugin::{CommandCompletion, Host};

use crate::plugin::{GoArrow, Here, display_title, say};

/// The verb's description in the client's help.
pub const DESCRIPTION: &str = "Set and control GoArrow destinations and navigation.";

/// `/go file`'s usage, also its answer when the file does not read.
pub const FILE_USAGE: &str = "GoArrow: Usage: /go file filename.xml (file not found or invalid).";

/// The help, one chat line each (GoArrow posted them as one message; note
/// `/go search` twice, as GoArrow listed it).
pub const HELP: [&str; 23] = [
    "GoArrow Commands:",
    "  /go <destination> - Set route to a named location",
    "  /go list - List all known locations",
    "  /go search <term> - Search locations",
    "  /go loc - Show current coordinates",
    "  /go dest - Show destination coordinates",
    "  /go file filename.xml - Load XML from the GoArrow storage directory",
    "  /go update [url] - Download location data, optionally changing the URL",
    "  /go url <url> - Set and persist the location-data URL",
    "  /go search <term> - Search locations",
    "  /go status - Show current destination",
    "  /go route - Show the current route steps",
    "  /go block <step> [note] - Never use a faulty route step again",
    "  /go corrections [path|reload|save] - Show, reload or save route corrections",
    "  /go dungeon [on|off|toggle|path|reload] - Show or hide the dungeon map indoors",
    "  /go dungeon export|import [filename.json] - Share learned dungeon paths",
    "  /go mark <name> - Save this indoor point as a named location",
    "  /go stop - Stop navigation",
    "  /go resume - Resume after a portal or recall interaction",
    "  /go clear - Clear destination",
    "  /go save <name> - Save location as favorite",
    "  /go favorites - List favorites",
    "  /go help - Show this help",
];

/// The subcommands completion offers (GoArrow's list).
pub const SUBCOMMANDS: [&str; 24] = [
    "to",
    "from",
    "start",
    "end",
    "selected",
    "attach",
    "list",
    "search",
    "loc",
    "dest",
    "mark",
    "route",
    "status",
    "stop",
    "resume",
    "clear",
    "reset",
    "lock",
    "unlock",
    "recall",
    "save",
    "favorites",
    "update",
    "help",
];

/// OpenAC's `PluginCommandLineParser.Parse`: arguments split by white
/// space; single or double quotes keep white space in one argument; a
/// backslash takes the next character as it is.
///
/// # Errors
/// A quote opened and never closed.
pub fn parse_arguments(text: &str) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    if text.trim().is_empty() {
        return Ok(result);
    }
    let mut token = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in text.chars() {
        if escaped {
            token.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                token.push(c);
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            continue;
        }
        if c.is_whitespace() {
            if !token.is_empty() {
                result.push(std::mem::take(&mut token));
            }
            continue;
        }
        token.push(c);
    }
    if escaped {
        token.push('\\');
    }
    if quote.is_some() {
        return Err("Unterminated quoted command argument.".to_owned());
    }
    if !token.is_empty() {
        result.push(token);
    }
    Ok(result)
}

fn go_to_state_name(state: GoToState) -> &'static str {
    match state {
        GoToState::None => "None",
        GoToState::Planning => "Planning",
        GoToState::Walking => "Walking",
        GoToState::Arrived => "Arrived",
        GoToState::NoRoute => "NoRoute",
        GoToState::Blocked => "Blocked",
        GoToState::Stopped => "Stopped",
        GoToState::Interrupted => "Interrupted",
        GoToState::Lost => "Lost",
        GoToState::Waiting => "Waiting",
        GoToState::ArrivedWithoutSight => "ArrivedWithoutSight",
    }
}

/// Runs `/go <arguments>`.
#[allow(clippy::too_many_lines)]
pub fn run(app: &mut GoArrow, host: &mut Host<'_>, arguments: &str) {
    let args = match parse_arguments(arguments) {
        Ok(args) => args,
        Err(error) => {
            say(host, &format!("GoArrow: {error}"));
            return;
        }
    };
    let Some(first) = args.first() else {
        help(host);
        return;
    };
    let rest = |from: usize| args.get(from..).unwrap_or_default().join(" ");
    match first.to_lowercase().as_str() {
        "help" | "?" => help(host),
        "list" => list(app, host),
        "search" | "find" => search(app, host, &rest(1)),
        "to" => to(app, host, &rest(1)),
        "from" | "start" => {
            let text = GoArrow::current_position_text(host, "GoArrow: Route origin");
            say(host, &text);
        }
        "end" => {
            app.stop_navigation(host);
            say(
                host,
                "GoArrow: Current route leg ended; use /go resume to continue.",
            );
        }
        "reset" => {
            app.clear_destination(host);
            say(host, "GoArrow: Route reset.");
        }
        "lock" => {
            app.set_navigation_lock(host, true);
            say(host, "GoArrow: Navigation locked.");
        }
        "unlock" => {
            app.set_navigation_lock(host, false);
            say(host, "GoArrow: Navigation unlocked.");
        }
        "selected" | "attach" => {
            let line = if app.set_selected_object_destination(host) {
                "GoArrow: Selected object attached as destination."
            } else {
                "GoArrow: Selected object is unavailable or has no position."
            };
            say(host, line);
        }
        "mark" => {
            let name = rest(1).trim().to_owned();
            let line = if app.mark_indoor(host, &name) {
                format!("GoArrow: Saved indoor location '{name}'.")
            } else {
                "GoArrow: Stand at an indoor point and use /go mark <name>.".to_owned()
            };
            say(host, &line);
        }
        "loc" => {
            let text = GoArrow::current_position_text(host, "GoArrow: Current position");
            say(host, &text);
        }
        "dest" => say(host, &app.destination_position_text()),
        "file" => {
            // The answer comes once the file is read (a few ticks).
            if args.len() != 2 || !app.load_data_file(host, &args[1]) {
                say(host, FILE_USAGE);
            }
        }
        "update" | "download" => {
            if args.len() > 1 && !app.set_external_data_url(host, &args[1]) {
                say(
                    host,
                    "GoArrow: URL must be an absolute http:// or https:// URL.",
                );
                return;
            }
            app.update_data(host);
        }
        "url" => {
            if args.len() != 2 || !app.set_external_data_url(host, &args[1]) {
                say(host, "GoArrow: Usage: /go url <http:// or https:// URL>");
                return;
            }
            say(
                host,
                &format!("GoArrow: Location-data URL set to '{}'.", args[1]),
            );
        }
        "stop" | "cancel" => {
            app.stop_navigation(host);
            say(host, "GoArrow: Navigation stopped.");
        }
        "resume" => app.resume(host),
        "status" => status(app, host),
        "route" | "steps" => route(app, host),
        "block" => match args.get(1).and_then(|n| n.trim().parse::<i64>().ok()) {
            Some(number) => {
                let line = match usize::try_from(number - 1) {
                    Ok(index) => app.block_route_step(host, index, &rest(2)),
                    Err(_) => format!("GoArrow: There is no route step {number}."),
                };
                say(host, &line);
            }
            None => say(host, "GoArrow: Usage: /go block <step number> [note]"),
        },
        "corrections" => {
            let mode = args
                .get(1)
                .map_or_else(|| "path".to_owned(), |m| m.to_lowercase());
            let line = match mode.as_str() {
                "reload" => app.load_corrections(host),
                "path" => GoArrow::corrections_path(host),
                "save" => app.save_observed_arrival(host),
                _ => "GoArrow: Usage: /go corrections [path|reload|save]".to_owned(),
            };
            say(host, &line);
        }
        "dungeon" => dungeon(app, host, &args),
        "favorites" | "favs" => {
            if app.favorites().is_empty() {
                say(host, "GoArrow: No favorites saved. Use /go save <name>");
            } else {
                let line = format!("GoArrow Favorites: {}", app.favorites().join(", "));
                say(host, &line);
            }
        }
        "save" => match args.get(1) {
            Some(name) => {
                app.add_favorite(host, name);
                say(host, &format!("GoArrow: '{name}' saved to favorites."));
            }
            None => say(host, "GoArrow: Usage: /go save <name>"),
        },
        "clear" => {
            app.clear_destination(host);
            say(host, "GoArrow: Destination cleared.");
        }
        "recall" => {
            let kind = args.get(1).map_or("lifestone", String::as_str).to_owned();
            app.recall(host, &kind);
        }
        _ => {
            // Coordinates may be typed unquoted: /go 42.1N 33.6E.
            let all = args.join(" ");
            if app.try_set_coordinate_destination(host, &all) {
                say(host, "GoArrow: Coordinate destination set.");
                return;
            }
            let name = all.trim();
            if name.is_empty() {
                return;
            }
            if app.set_destination_by_name(host, name) {
                say(host, &format!("GoArrow: Destination set to '{name}'."));
            } else {
                say(
                    host,
                    &format!("GoArrow: Destination '{name}' not found in location database."),
                );
            }
        }
    }
}

fn help(host: &mut Host<'_>) {
    for line in HELP {
        say(host, line);
    }
}

fn to(app: &mut GoArrow, host: &mut Host<'_>, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        say(host, "GoArrow: Usage: /go to <location|coordinates|here>");
        return;
    }
    if text.eq_ignore_ascii_case("here") {
        let line = GoArrow::current_position_text(host, "GoArrow: Current position");
        say(host, &line);
        return;
    }
    if app.try_set_coordinate_destination(host, text) || app.set_destination_by_name(host, text) {
        say(host, "GoArrow: Destination set.");
    } else {
        say(host, &format!("GoArrow: Destination '{text}' not found."));
    }
}

fn list(app: &GoArrow, host: &mut Host<'_>) {
    let names = app.all_destination_names();
    if names.is_empty() {
        say(host, "GoArrow: No locations loaded.");
        return;
    }
    let shown: Vec<&str> = names.iter().take(20).map(String::as_str).collect();
    say(host, &format!("GoArrow Locations: {}", shown.join(", ")));
    if names.len() > 20 {
        say(host, &format!("... and {} more.", names.len() - 20));
    }
}

fn search(app: &GoArrow, host: &mut Host<'_>, query: &str) {
    if query.trim().is_empty() {
        say(host, "GoArrow: Usage: /go search <term>");
        return;
    }
    let matches = app.search_locations(query);
    if matches.is_empty() {
        say(host, &format!("GoArrow: No locations match '{query}'."));
        return;
    }
    let shown: Vec<&str> = matches.iter().take(20).map(|l| l.name.as_str()).collect();
    say(host, &format!("GoArrow matches: {}", shown.join(", ")));
    if matches.len() > 20 {
        say(host, &format!("... and {} more.", matches.len() - 20));
    }
}

fn status(app: &mut GoArrow, host: &mut Host<'_>) {
    let here = Here::read(host);
    let place = if !here.available {
        "position unavailable"
    } else if here.portal_space {
        "portal space"
    } else if here.position.is_outdoor {
        "outdoors"
    } else {
        "indoors"
    };
    let step = app
        .route_step_lines()
        .into_iter()
        .next()
        .unwrap_or_else(|| "no active step".to_owned());
    let report = host.go_to_report();
    let state = go_to_state_name(report.state);
    let client = match report.reason.as_deref().map(str::trim) {
        Some(reason) if !reason.is_empty() => format!("{state} ({reason})"),
        _ => state.to_owned(),
    };
    say(
        host,
        &format!(
            "{}: {}; {place}; client {client}; next {step}.",
            display_title(),
            app.nav_status_text(&here)
        ),
    );
    if let Some(reason) = app
        .navigator()
        .view()
        .failure_reason
        .clone()
        .filter(|r| !r.is_empty())
    {
        say(host, &format!("GoArrow: Reason: {reason}"));
    }
    let lines = app.nav(host, |n, cx| n.dungeon_portal_diagnostics(cx));
    for line in lines {
        say(host, &format!("GoArrow: {line}"));
    }
    let recalls = host.recalls();
    if recalls.is_available {
        let known = recalls.locations.iter().filter(|l| l.is_known).count();
        say(
            host,
            &format!("GoArrow: Recall destinations known: {known}."),
        );
    }
    let destination = app.destination().target_name().to_owned();
    if destination.is_empty() {
        say(host, "GoArrow: No destination set. Use /go <name>");
    } else {
        say(
            host,
            &format!("GoArrow: Destination '{destination}'. Use /go stop to cancel."),
        );
    }
}

fn route(app: &GoArrow, host: &mut Host<'_>) {
    let steps = app.route_step_lines();
    if steps.is_empty() {
        say(host, "GoArrow: No route is currently calculated.");
        return;
    }
    say(host, "GoArrow route:");
    for (i, step) in steps.iter().enumerate() {
        say(host, &format!("  {}. {step}", i + 1));
    }
}

fn dungeon(app: &mut GoArrow, host: &mut Host<'_>, args: &[String]) {
    let mode = args
        .get(1)
        .map_or_else(|| "toggle".to_owned(), |m| m.to_lowercase());
    if mode == "export" || mode == "import" {
        let line = if args.len() <= 3 {
            app.share_traversals(host, mode == "export", args.get(2).map(String::as_str))
        } else {
            "GoArrow: Usage: /go dungeon export|import [filename.json]".to_owned()
        };
        say(host, &line);
        return;
    }
    if args.len() > 2 || !matches!(mode.as_str(), "on" | "off" | "toggle" | "path" | "reload") {
        say(
            host,
            "GoArrow: Usage: /go dungeon [on|off|toggle|path|reload]",
        );
        return;
    }
    if mode == "path" || mode == "reload" {
        // The map draws the dungeon from the game's own data; there is no
        // folder of images (DIVERGENCES.md, GA-16).
        say(
            host,
            "GoArrow: The dungeon map is drawn from the game's own data; there are no map files to load.",
        );
        return;
    }
    let visible = if mode == "toggle" {
        !app.settings().dungeon_map_visible
    } else {
        mode == "on"
    };
    app.set_dungeon_map_visible(host, visible);
    say(
        host,
        &format!(
            "GoArrow: Dungeon map {}.",
            if visible { "enabled" } else { "disabled" }
        ),
    );
}

/// Completions for what follows `/go`: a subcommand, or after `to` and
/// `search` up to 20 places with their notes.
#[must_use]
pub fn complete(app: &GoArrow, arguments: &str) -> Vec<CommandCompletion> {
    let Ok(args) = parse_arguments(arguments) else {
        return Vec::new();
    };
    let prefix = args.last().map_or("", String::as_str);
    if args.len() <= 1 {
        let prefix = prefix.to_lowercase();
        return SUBCOMMANDS
            .iter()
            .filter(|s| s.starts_with(&prefix))
            .map(|s| CommandCompletion {
                text: (*s).to_owned(),
                description: None,
            })
            .collect();
    }
    let first = args[0].to_lowercase();
    if first == "to" || first == "search" {
        return app
            .search_locations(prefix)
            .iter()
            .take(20)
            .map(|l| CommandCompletion {
                text: l.name.clone(),
                description: Some(l.notes.clone()),
            })
            .collect();
    }
    Vec::new()
}
