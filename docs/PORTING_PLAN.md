# GoArrow → OpenAC Porting Plan

## Purpose and boundary

This document records what the GoArrow port currently does, where its behavior
is limited, and what remains for a closer replacement of the original plugin.

The plugin uses the supported `AcDream.Plugin.Abstractions` contract. It must
not depend on OpenAC `App`, `Runtime`, or `Core` internals, Decal APIs, raw
network messages, client memory, packet injection, or graphics-device types.

This status reflects the repository on 2026-09-24. "Implemented" means the
behavior exists in this repository, not that every original GoArrow workflow
has been reproduced or verified in a live client.

## References

- Original GoArrow/VVSEdition reference: <https://github.com/kaldorgreybear/AsheronsCall-VTGoArrow>
- Original GoArrow documentation: <http://virindi.net/wiki/index.php/GoArrow_(VVS_Edition)>
- OpenAC repository: <https://github.com/eriknihlen/OpenAC>
- OpenAC plugin development documentation: <https://github.com/eriknihlen/OpenAC/tree/main/docs>
- Historical OpenAC API requests: `docs/OpenAC-improvements.md`. Check the
  current contract before treating any request there as an API gap.

## Current implementation

| Area | Implemented in this repository | Remaining limit |
| --- | --- | --- |
| Plugin boundary | .NET 10 `IAcDreamPlugin`, manifest, lifecycle, optional headless behavior, declarative panel | Validate the packaged plugin against the minimum supported host release and in a live graphical session |
| Destinations | Named locations, coordinates, selected objects, route steps, clicked chat coordinates; object updates and disappearance handling | Coordinate and object destinations do not restore correctly after restart; richer original attach/tag workflows remain |
| Commands | `/go` and `/goarrow`, quoted arguments, typed command definition, subcommand/location completion, search, favorites, route control, recall, data and dungeon commands | `/go from` and `/go start` only report the current position; `/go to here` reports it rather than setting a destination; help text is maintained separately from command metadata |
| Location data | Rich location metadata, compact GoArrow and OpenAC XML, Atlas XML, embedded defaults, user indoor marks, local file loading, resource-catalog loading | Layer precedence and conflict rules need a documented, tested policy; imported Atlas metadata is incomplete |
| Route finding | Weighted graph, portal-device and route-start edges, direct-walk fallback, four cost profiles, route-step editing methods | Multiple candidate routes, explanations, action availability constraints, and atomic database/graph snapshot replacement remain |
| Walking | `GoTo` legs, ownership/sequence/revision checks, blocked-door activation, failure reporting, portal transition recovery | Interrupted and blocked walking mostly stops with a diagnostic; retry/replan policy and interaction timeouts are incomplete |
| Interactions | Nearby portal matching and activation, activation reports, transition-based continuation, manual `/go resume` fallback | NPC/dialog actions, portal usage requirements, and unambiguous correlation of every interaction to its route leg remain |
| Recall | Semantic recall calls for lifestone, marketplace, house, mansion, and allegiance; known-location capture and successful transition learning scoped by character/world | Full bind/tie state, stale/unknown/unavailable display, and safe recall-edge availability policy remain |
| UI | Searchable From/Destination fields, route list and details, indoor path length and cell-aware waypoints, basic navigation status, data URLs and download actions, visibility controls | Detailed progress/failure bindings, route-step editing controls, route cost/profile editor, saved route profiles, consistent validation feedback, and complete command/panel parity remain |
| HUD and maps | Arrow and compact toolbar canvases, Dereth map surface with markers/route lines, click-to-coordinate, optional schematic dungeon canvas | Original artwork/tooltips and toolbar actions; calibrated dungeon player/route overlays, floor focus, and dungeon click navigation |
| Storage and updates | `settings.json` with legacy-key migration, explicit Atlas XML download and validated cache, optional dungeon ZIP download with validation | Scoped state audit, migration/version diagnostics, cancellable UI operations, robust snapshot replacement, provenance and redistribution decisions |
| Tests and release | Route, data, destination, navigation, UI, HUD, map, dungeon, lifecycle, and command tests; CI build/test and release packaging | Live-host smoke coverage and a documented release gate for the packaged archive |

### Important behavior already ported

- The destination model distinguishes location, coordinates, object, route,
  and recall kinds. `/go selected` attaches the selected object, and object
  change events update or invalidate that target.
- `PluginChatCoordinateLinkRouter` handles clicked coordinate links.
- `GoArrowNavigator` can walk to an exact indoor cell position or a matching
  live portal in the current indoor area. Outdoor graph routing pauses in
  portal space or indoors when no indoor leg can be resolved. `/go mark <name>`
  saves an exact indoor point in `GoArrow/indoor-locations.xml`.
- The Route tab uses `PreviewPathAsync` to show plan-only indoor walking
  distance and ordered cell-aware waypoints for an available indoor target.
- The Dereth map uses OpenAC map resources when available. Dungeon diagrams
  come from an optional user-supplied ZIP or extracted images and are selected
  by indoor landblock. They support zoom and pan, but the source diagrams do
  not provide floor regions or a pixel-to-world transform.
- Atlas and dungeon downloads are opt-in. Failed Atlas downloads can fall
  back to a validated cache; failed dungeon downloads retain the previous
  archive. Neither download starts automatically.

## Remaining work, in priority order

### 1. Restore and scope destination state

Settings currently save `DestinationName`. On startup the plugin resolves that
string only as a database location. A coordinate destination saves its display
text there, and an object destination saves its object name, so neither kind
reliably returns after restart.

1. Persist destination kind and the data needed to restore that kind. Define
   whether an object destination should reattach to a live object, become
   unavailable, or be cleared after logout.
2. Migrate the existing `DestinationName` setting without losing named
   destinations. Retain the original coordinate text for display.
3. Define per-character versus global scope for destination, route origin,
   favorites, indoor marks, HUD/map state, and recall state. Prevent one
   character or world from inheriting another's authoritative recall data.
4. Test restart, character/world switch, missing location, and vanished object
   behavior through the plugin lifecycle.

### 2. Make multi-leg navigation and interactions predictable

Walking reports already filter foreign owners and stale sequence/revision
values. Portal activation and transition recovery work for supported routes,
with manual resume when an interaction cannot be identified.

1. Define a route-leg identity and correlate navigation, activation, recall,
   and transition events to that leg. Reject unrelated or late events.
2. Apply bounded retry, timeout, and replan policies after interruption,
   blocked movement, target movement, failed activation, and portal-space exit.
   Expose the final reason and recovery action in chat and the panel.
3. Add portal usage restrictions and explicit action availability to route
   selection. Do not choose an action merely because its edge is cheap.
4. Add NPC/dialog actions where the original route data actually requires
   them. Keep manual `/go resume` for unsupported or ambiguous interactions.
5. Cover indoor cases: exact cell destinations, same-dungeon selected objects,
   named live portal matching, Town Network continuation, stale indoor targets,
   and the pause/resume boundary between indoor and outdoor routing.

### 3. Make data replacement and provenance explicit

The plugin loads embedded data, cached Atlas data, resource-catalog files,
and saved indoor locations. Database loads replace base location collections;
user indoor locations are reapplied. The route finder invalidates its graph
after updates and rebuilds it lazily. This is not a single immutable
database-plus-graph snapshot.

1. Specify source order and record conflict rules by stable ID and name,
   including duplicate names, retired records, user overrides, and invalid
   records. Test the rules across all input formats.
2. Build and validate replacement location, portal, route-start, and graph
   state before publishing it as one coherent snapshot. Preserve the last
   valid snapshot after a failed update.
3. Add schema/version diagnostics for migrated data and report which source
   supplied a location or route edge.
4. Finish Atlas field mapping and document unsupported restrictions before
   using imported records for automatic interactions.
5. Add user-visible cancellation and progress for Atlas and dungeon downloads.
   Decide how cache age affects startup, offline fallback, and refresh; the
   current `AtlasCacheMaxAgeDays` setting is used for failed-download fallback.
6. Record external data source and license/attribution requirements before
   redistributing downloaded XML, dungeon images, or copied original assets.

### 4. Finish recall and route policy

Semantic recall requests and successful-transition learning exist. A known
recall location is not the same as a currently usable recall action.

1. Track primary/secondary portal ties, lifestone bind, house/mansion, and
   allegiance state only through supported host signals. Label unknown,
   stale, and unavailable values distinctly in `/go status` and the panel.
2. Add recall route-start edges only when both destination and action are
   usable. Correlate completion with the request revision before learning.
3. Expose the existing cost profiles in the UI, and test deterministic edge
   selection with unavailable actions, portal restrictions, and alternate
   routes. Add route explanations or alternatives if needed for parity.

### 5. Complete command, panel, and map workflows

1. Decide and document the intended semantics of `/go from`, `/go start`,
   `/go to here`, `/go end`, and `/go reset`; make command help, completion,
   panel actions, and tests agree. Preserve `/go` as the OpenAC command prefix.
2. Show validation errors and detailed leg progress/failure reasons in the
   panel. Bind the existing route-step editing methods to controls. Add route
   cost/profile selection, saved route profiles, and panel focus/visibility
   controls where useful.
3. Keep Dereth map acceptance separate from dungeon diagrams. Dereth clicks
   can create coordinate destinations; dungeon clicks and overlays require
   calibrated floor regions and pixel-to-world mapping. Until that data
   exists, keep dungeon diagrams explicitly schematic.
4. Decide which original arrow/toolbar artwork, tooltips, and actions are
   worth reproducing through public canvas/resource APIs.

### 6. Verify the released plugin

1. Keep deterministic fake-host tests for lifecycle, commands, destinations,
   route data, walking, interactions, recall, maps, storage, and headless mode.
2. Add an end-to-end fixture for walking, portal activation, transition,
   indoor continuation, and final arrival. Include foreign and delayed event
   reports and an update during route planning.
3. For each release, build against the minimum supported OpenAC version,
   validate the archive with the host's plugin checker, and smoke-test the
   installed archive in a graphical client. Check panel/canvas input,
   transition recovery, persistence across restart, and disable/unload.

## Completion criteria

The port can be described as a complete behavioral replacement only when it
provides equivalent destination, route, command, panel, chat, HUD, and map
workflows through supported OpenAC APIs; handles unavailable actions and live
world changes safely; restores scoped user state; and passes both automated
and packaged-host checks. Schematic dungeon maps remain a documented limit
until calibrated map data is available.

Until then, describe it as a supported navigation port with the specific
limits above, rather than as a full replacement for the original GoArrow.
