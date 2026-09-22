# GoArrow → OpenAC Implementation Plan

## Purpose

`docs/OpenAC-improvements.md` states that the OpenAC contract now provides all capabilities required for a complete GoArrow port. `docs/PORTING_PLAN.md` describes the remaining behavioral gaps in the GoArrow plugin. This document turns those gaps into an implementation plan.

The completed port must use only `AcDream.Plugin.Abstractions`; it must not reference OpenAC `App`, `Runtime`, or `Core` internals, Decal APIs, raw packets, client memory, packet injection, graphics-device types, or platform-specific window APIs.

## Current baseline

The GoArrow repository already contains:

- plugin lifecycle, manifest, declarative panel, settings persistence, and `/go` registration;
- embedded location, portal-device, and route-start data;
- compact GoArrow, OpenAC, and Warcry Atlas XML parsing;
- location search, favorites, coordinate calculations, and weighted shortest-path routing;
- multi-hop walking, route-start edges, portal metadata, and direct-walk fallback;
- navigation report handling, request sequence filtering, failure handling, and manual interaction pauses;
- opt-in validated external data download with cache fallback;
- OpenAC support for rendering, maps, structured chat links, typed commands, object events, interactions, recall state, scoped storage, resources, UI input, and test fixtures.

The primary remaining work is wiring these available abstractions into GoArrow and completing the associated tests.

---

## Phase 1 — Destination and chat parity

### Objectives

Support all original destination entry paths: named locations, coordinates, selected objects, and route destinations.

### Implementation

1. Add an explicit destination model with kinds:
   - `Coordinates`;
   - `Location`;
   - `Object`;
   - `Route`;
   - recall destinations where applicable.
2. Add coordinate destinations to `GoArrowDestination`, retaining coordinate text for display and persistence.
3. Replace `Arguments.Split(' ')` in `GoArrowCommands` with `PluginCommand.ParseArguments()`.
4. Add `/go to <location|coordinates|here>`.
5. Add `/go from` and `/go start` to set or report the route origin.
6. Add `/go end`, `/go reset`, `/go lock`, and `/go unlock` as compatible commands.
7. Wire `PluginChatCoordinateLinkRouter` during `Enable()` and route clicked coordinates to a GoArrow destination.
8. Subscribe to `ISelectionService.Changed` and add object-target commands such as `/go selected` or `/go attach`.
9. Handle selected-object removal or disappearance by marking the target unavailable rather than silently retaining stale coordinates.
10. Convert GoArrow commands to `IPluginCommandDefinition` registrations with:
    - aliases;
    - descriptions;
    - quoted arguments;
    - generated help;
    - location and subcommand completion.

### Files

- `src/AcDream.Plugins.GoArrow/GoArrowDestination.cs`
- `src/AcDream.Plugins.GoArrow/GoArrowCommands.cs`
- `src/AcDream.Plugins.GoArrow/GoArrowPlugin.cs`
- new destination-kind and coordinate-target tests

### Acceptance criteria

- `/go "multi word location"` resolves correctly;
- `/go to 42.1N 33.6E` creates a coordinate destination;
- clicking a coordinate chat link sets the destination;
- selected objects can become destinations and stale objects are handled safely;
- completion returns matching locations without blocking the host;
- all command aliases and quoted arguments are covered by tests.

---

## Phase 2 — Interaction-driven navigation

### Objectives

Replace manual interaction pauses with semantic portal, door, NPC, and object actions.

### Implementation

1. Extend route steps with interaction metadata:
   - object ID when known;
   - object capability;
   - activation action;
   - expected completion condition;
   - timeout and retry policy.
2. Discover nearby portal and door objects through `INavigationAutomation.CaptureObjects()` and `TryGetObject()`.
3. Activate portal devices through `IWorldObjectAutomation.Activate()`.
4. Subscribe to `IEvents.ActivationCompleted` and correlate reports by request/revision.
5. Advance the route only for successful activation outcomes.
6. Treat `TargetLost`, `Refused`, `Blocked`, `Interrupted`, and `TimedOut` as explicit route failures with diagnostics.
7. Automatically activate doors when a walking report contains `BlockedByObjectId` and the object is a closed, interactable door.
8. Add NPC and dialog actions where route data requires them.
9. Preserve manual `/go resume` as a fallback for unsupported or ambiguous interactions.
10. Subscribe to `IEvents.PortalTransition` and use completed transitions to resume the route automatically.

### Files

- `src/AcDream.Plugins.GoArrow/GoArrowNavigator.cs`
- `src/AcDream.Plugins.GoArrow/RouteFinding/Route.cs`
- `src/AcDream.Plugins.GoArrow/RouteFinding/RouteFinder.cs`
- `src/AcDream.Plugins.GoArrow/RouteFinding/PortalDevice.cs`

### Acceptance criteria

- a portal route leg activates the correct object and advances after completion;
- failed and stale activation reports never advance the route;
- closed doors can be opened after a blocked navigation report;
- object disappearance produces a recoverable diagnostic;
- completed portal transitions resume navigation from the new position;
- manual resume remains functional in headless or unsupported hosts.

---

## Phase 3 — Recall, house, and allegiance destinations

### Objectives

Use authoritative OpenAC recall state instead of placeholders or chat-only inference.

### Implementation

1. Replace the current `/go recall` placeholder with `IRecallAutomation.Recall()`.
2. Support:
   - `/go recall lifestone`;
   - `/go recall marketplace`;
   - `/go recall house`;
   - `/go recall mansion`;
   - `/go recall allegiance`.
3. Read `IRecallAutomation.CaptureLocations()` and distinguish known, unknown, stale, and unavailable destinations.
4. Correlate recall request revisions with `PluginPortalTransition.RecallRequestRevision`.
5. Learn and persist successful recall locations using character/world scope.
6. Add recall route-start edges only when the destination is known and usable.
7. Display recall availability and source status in `/go status` and the panel.
8. Add explicit failure handling for unavailable, unsupported, refused, cancelled, and interrupted recall requests.

### Files

- `src/AcDream.Plugins.GoArrow/GoArrowCommands.cs`
- `src/AcDream.Plugins.GoArrow/GoArrowNavigator.cs`
- `src/AcDream.Plugins.GoArrow/GoArrowDestination.cs`
- `src/AcDream.Plugins.GoArrow/GoArrowPanel.cs`
- `src/AcDream.Plugins.GoArrow/GoArrowSettings.cs`

### Acceptance criteria

- recall commands invoke the correct semantic host operation;
- locations are learned only after successful transitions;
- state is scoped to the correct character and world;
- unknown values are never presented as authoritative;
- recall route legs resume or fail based on correlated transition events.

---

## Phase 4 — Navigation reliability and route policies

### Objectives

Make multi-leg route execution robust against interference, interruption, blocked paths, and changing world state.

### Implementation

1. Verify `PluginGoToReport.Owner` and sequence before processing reports.
2. Track route ID, leg ID, request sequence, report revision, and transition generation.
3. Reject duplicate and stale reports across all navigation and interaction handlers.
4. Replan after player interruption once the character is stationary.
5. Replan after portal-space exit using the confirmed navigation snapshot.
6. Detect target movement through `ObjectChanged` and update object-target routes.
7. Add configurable route cost profiles:
   - shortest walk;
   - fewest interactions/portals;
   - prefer recall;
   - avoid unavailable actions.
8. Rebuild route graphs atomically after data replacement.
9. Add configurable retry and timeout policies for walking and interactions.
10. Expose detailed current-leg progress and failure reason to the panel.

### Files

- `src/AcDream.Plugins.GoArrow/GoArrowNavigator.cs`
- `src/AcDream.Plugins.GoArrow/RouteFinding/RouteGraph.cs`
- `src/AcDream.Plugins.GoArrow/RouteFinding/RouteFinder.cs`
- `src/AcDream.Plugins.GoArrow/GoArrowSettings.cs`

### Acceptance criteria

- another plugin or player command cannot be mistaken for GoArrow navigation;
- interruption causes controlled replan rather than permanent failure;
- portal-space transitions do not trigger false arrivals;
- route data replacement never exposes a partially rebuilt graph;
- route profiles produce deterministic, tested edge selection.

---

## Phase 5 — Arrow and toolbar HUDs

### Objectives

Restore the original directional arrow, floating toolbar, and status overlays.

### Implementation

1. Add a `GoArrowHud` component using `IPluginRenderRegistry`.
2. Register an arrow HUD with persisted bounds, visibility, scale, and click-through settings.
3. Render:
   - directional arrow based on destination bearing;
   - distance and destination text;
   - navigation state;
   - interaction/waiting indicators.
4. Register a toolbar HUD with stop, resume, clear, panel, and map actions.
5. Load icons through `IPluginResources` or `IPluginRenderRegistry.LoadTexture()`.
6. Route HUD input safely:
   - click arrow to stop or open panel according to settings;
   - click toolbar buttons to invoke owned actions;
   - do not interfere with the game when click-through is enabled.
7. Dispose all HUDs, textures, and callbacks during `Disable()`.
8. Provide inert behavior when rendering is unavailable.

### Files

- new `src/AcDream.Plugins.GoArrow/GoArrowHud.cs`
- `src/AcDream.Plugins.GoArrow/GoArrowPlugin.cs`
- `src/AcDream.Plugins.GoArrow/GoArrowSettings.cs`
- `plugin.json` and new HUD assets

### Acceptance criteria

- arrow orientation changes when bearing changes;
- HUD bounds and visibility survive restart;
- toolbar actions invoke only GoArrow operations;
- unload releases registrations and textures;
- headless hosts load without exceptions;
- rendering and input behavior have deterministic fake-surface tests.

---

## Phase 6 — Dereth and dungeon map HUDs

Current progress: the Dereth surface and optional schematic dungeon map canvas
are implemented. Players supply a ZIP or extracted images in the persistent
`dungeon-maps` folder and can reload them with `/go dungeon reload`. Dungeon
images are selected from the indoor cell's landblock ID and can be zoomed or
panned. The source images combine floors and do not
contain a pixel-to-world transform, so floor focus and player/route overlays
on dungeon diagrams remain open items below.

### Objectives

Restore map windows with current position, destination, route lines, portal markers, zoom, pan, and click-to-navigate.

### Implementation

1. Add a `GoArrowMap` component using `IPluginMapRegistry` and `IPluginMapResourceCatalog`.
2. Register a Dereth map and load its background asynchronously with cancellation.
3. Register dungeon/floor maps and select the correct map based on `IsOutdoor`, dungeon metadata, and floor identity.
4. Use canonical `PluginMapPoint` values and `LinearPluginMapCoordinateConverter` or a data-specific converter.
5. Publish markers for:
   - current position;
   - destination;
   - route waypoints;
   - portal entrances and exits;
   - search results.
6. Publish route lines from the current route.
7. Handle map click, wheel, drag, marker selection, zoom, and pan.
8. Convert map clicks back into coordinate destinations.
9. Persist map viewport and visibility per character or globally as configured.
10. Implement missing-map and missing-tile fallbacks.

### Files

- new `src/AcDream.Plugins.GoArrow/GoArrowMap.cs`
- `src/AcDream.Plugins.GoArrow/GoArrowPlugin.cs`
- `src/AcDream.Plugins.GoArrow/GoArrowSettings.cs`
- map resource declarations and assets

### Acceptance criteria

- world/map conversion round-trips within documented tolerance;
- current and destination markers remain correct while panning/zooming;
- route lines update after recalculation;
- clicking the map creates a coordinate destination;
- dungeon maps switch correctly by region/floor;
- asynchronous loading never blocks the UI or crashes on missing assets.

---

## Phase 7 — Declarative panel parity

Current progress: the panel has searchable From and Destination fields,
clickable matches, a Current Location choice in both fields, and a route-step
list. A named From location previews a route without starting movement.

### Objectives

Make the panel a complete alternative to chat commands.

### Implementation

1. Extend `GoArrowPanel` from `PluginPanelBinding`.
2. Add destination text input with submit, validation, and autocomplete.
3. Add location search results with click-to-select behavior.
4. Add a virtualized route-step list with stable row keys.
5. Display active leg, progress, interaction state, and failure diagnostics.
6. Add controls for route profile, map/HUD visibility, recalculation, and data updates.
7. Add route editing:
   - remove leg;
   - reorder leg;
   - save route profile;
   - restore route profile.
8. Invalidate bindings on meaningful state changes rather than relying solely on polling.
9. Add panel visibility/focus actions from HUD and commands.

### Files

- `src/AcDream.Plugins.GoArrow/goarrow-panel.xml`
- `src/AcDream.Plugins.GoArrow/GoArrowPanel.cs`
- `src/AcDream.Plugins.GoArrow/GoArrowPlugin.cs`
- new panel view-model/helper types as needed

### Acceptance criteria

- entering a location shows suggestions and sets the destination on submit;
- lists preserve selection across refreshes;
- route progress updates after navigation reports;
- validation errors are visible and do not throw;
- hidden panels stop unnecessary expensive updates;
- panel controls work in the fake host and remain inert headlessly.

---

## Phase 8 — Storage, layered data, and update workflow

### Objectives

Complete persistent route-data management and migration support.

### Implementation

1. Use structured/JSON storage for settings where supported, retaining migration from existing text keys.
2. Separate storage scopes for:
   - global settings;
   - per-character state;
   - per-world portal/recall data;
   - user route profiles.
3. Discover embedded, installed, and user data through `IPluginResourceCatalog.ListDataFiles()`.
4. Define and implement deterministic merge precedence.
5. Add schema versions and migration diagnostics for old GoArrow data.
6. Build new location/database/graph snapshots before atomic replacement.
7. Add cache expiry and configurable refresh policy for Atlas data.
8. Add cancellation, progress, timeout, and offline diagnostics to updates.
9. Preserve the last valid snapshot when an update fails.
10. Add attribution and source metadata to imported external data.

### Files

- `src/AcDream.Plugins.GoArrow/GoArrowSettings.cs`
- `src/AcDream.Plugins.GoArrow/RouteFinding/LocationDatabase.cs`
- `src/AcDream.Plugins.GoArrow/RouteFinding/RouteGraph.cs`
- `src/AcDream.Plugins.GoArrow/RouteFinding/WarcryAtlasDataProvider.cs`
- `src/AcDream.Plugins.GoArrow/GoArrowCommands.cs`

### Acceptance criteria

- interrupted writes preserve the previous valid file;
- character/world data cannot leak across scopes;
- malformed data is rejected with actionable diagnostics;
- graph readers see complete old or new snapshots only;
- failed downloads retain the last valid cache;
- updates can be cancelled without corrupting data.

---

## Phase 9 — Integration and regression testing

### Objectives

Prove lifecycle safety, host-boundary compliance, and complete behavior using deterministic fixtures.

### Test suites

1. **Lifecycle:** initialize, enable, disable, re-enable, and unload with every optional capability available and unavailable.
2. **Destination:** named, coordinate, object, selected-object, route, and recall destinations.
3. **Commands:** aliases, quoting, completion, generated help, invalid input, and command ownership.
4. **Chat:** coordinate parsing, link routing, malformed links, and disposal.
5. **Navigation:** ownership, sequence/revision filtering, arrivals, out-of-sight arrivals, blocked doors, interruptions, replans, portal transitions, and unload during navigation.
6. **Interactions:** portal, door, NPC, dialog, target loss, stale reports, failure, timeout, and cancellation.
7. **Recall:** request status, revision correlation, successful learning, unknown/stale state, and character/world scope.
8. **HUD:** registration, rendering commands, input, persistence, disposal, and headless behavior.
9. **Maps:** conversion, markers, routes, click-to-destination, zoom/pan, asynchronous loading, missing assets, and dungeon selection.
10. **Panel:** input, autocomplete, list selection, invalidation, route editing, and focus/visibility.
11. **Data:** parser validation, layered overrides, migration, cache fallback, atomic replacement, and cancellation.
12. **Full integration:** a route involving walking, portal activation, recall, portal transition recovery, and final arrival.

### Files

Add focused tests under `tests/AcDream.Plugins.GoArrow.Tests/`, using the OpenAC fake host and fixture implementations.

### Acceptance criteria

- all new capabilities have contract-level and GoArrow integration coverage;
- full solution build and test suite pass;
- headless operation remains exception-free;
- no test relies on OpenAC internals or live client state.

---

## Recommended execution order

1. Phase 1 — destination and chat parity.
2. Phase 2 — interaction-driven navigation.
3. Phase 3 — recall and semantic state.
4. Phase 9 tests for Phases 1–3.
5. Phase 4 — navigation reliability and route policies.
6. Phase 5 — arrow and toolbar HUDs.
7. Phase 7 — panel parity.
8. Phase 8 — storage and data workflow.
9. Phase 6 — maps and dungeon maps.
10. Phase 9 integration
