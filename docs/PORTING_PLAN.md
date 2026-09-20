# GoArrow → OpenAC Porting Plan

## Purpose

This document tracks the port of GoArrow to OpenAC's supported
`AcDream.Plugin.Abstractions` contract. It distinguishes:

- behavior currently implemented in this repository;
- behavior adapted because OpenAC exposes a different API;
- behavior not yet implemented;
- OpenAC capabilities that would be needed for a complete behavioral replacement.

The port must not depend on OpenAC `App`, `Runtime`, or `Core` internals, Decal APIs, raw network messages, client memory access, or packet injection.

## References

- Original GoArrow/VVSEdition reference: <https://github.com/kaldorgreybear/AsheronsCall-VTGoArrow>
- Original GoArrow documentation: <http://virindi.net/wiki/index.php/GoArrow_(VVS_Edition)>
- OpenAC repository: <https://github.com/eriknihlen/OpenAC>
- OpenAC plugin development documentation: <https://github.com/eriknihlen/OpenAC/tree/main/docs>
- External Atlas data source considered for future work: `http://maps.roogon.com/downloads/data_cod_TN_Directions_Non_Olthoi.xml`

## Current implementation status

### Implemented

- .NET 10 plugin project and OpenAC manifest.
- `IAcDreamPlugin` lifecycle through `GoArrowPlugin`.
- OpenAC-compatible `plugin.json`:
  - ID: `openac.goarrow`;
  - entry DLL: `AcDream.Plugins.GoArrow.dll`;
  - API version: `1`;
  - kind: `Gameplay`.
- Declarative `goarrow-panel.xml` panel.
- Settings persisted through `IPluginStorage.ReadText` and `WriteText`.
- `/go` command registration through `IPluginCommandRegistry`.
- Embedded default location, portal-device, and route-start data.
- Coordinate parsing, distance, bearing, rounding, and landcell conversion.
- Rich location model retaining:
  - ID;
  - `LocationType`;
  - primary and exit coordinates;
  - notes/descriptions;
  - dungeon ID;
  - favorite/customized/retired state;
  - `UseInRouteFinding`;
  - specialized icon metadata.
- Compact GoArrow XML parsing and serialization.
- Existing simplified OpenAC XML parsing.
- Crossroads/Warcry Atlas location parsing, including latitude sign conversion.
- Location database search and case-insensitive lookup.
- Basic portal-device and route-start records.
- Basic route construction and route finding.
- Optional `INavigationAutomation.GoTo` integration.
- Headless/unavailable navigation degradation.
- 96 automated route/data-model tests passing.

### Adapted

| Original feature | OpenAC implementation | Status |
| --- | --- | --- |
| `@go`/configurable command prefix | `/go` command registration | Adapted; OpenAC does not currently require legacy `@` support |
| Arrow D3D HUD | Declarative panel with destination, distance, bearing, and status | Adapted; custom rendering unavailable |
| Map/dungeon HUD | No equivalent map canvas currently | Deferred |
| Toolbar HUD | Panel actions and chat commands | Adapted |
| Manual key-held movement | `INavigationAutomation.GoTo` | Adapted |
| Decal settings | `IPluginStorage` text keys | Adapted |
| Decal XML services | .NET XML APIs and embedded resources | Adapted |
| Raw server dispatch | No raw dispatch; semantic OpenAC state only | Deferred/limited |
| Clickable chat coordinates | Manual `/go` commands | Deferred |

## Feature inventory and status

### 1. Destination tracking

Original behavior supports coordinates, named locations, selected objects, and route waypoints.

Current state:

- named locations are supported;
- destination state and route state are implemented;
- distance and bearing are calculated from the current position;
- object destinations are not yet fully wired to selection/object events;
- direct coordinate command parsing is not yet implemented in the command handler;
- route-waypoint advancement exists in the navigator but needs more robust host reports.

Next work:

1. Add explicit destination kinds (`Coordinates`, `Location`, `Object`, `Route`).
2. Wire `ISelectionService` and `IEvents.ObjectChanged` for object targets.
3. Add `/go to`, `/go here`, and coordinate parsing.
4. Add tests for every destination kind and loss of the target object.

### 2. Location model and data

The `Location` model must remain compatible with the original GoArrow data concepts. It must not be reduced to just `Name`, `NS`, and `EW`.

Retained metadata:

- stable source ID;
- semantic `LocationType`;
- primary coordinates;
- portal/dungeon exit coordinates;
- description/notes;
- dungeon ID;
- retired state;
- route-finding eligibility;
- favorite/customized state;
- specialized icon metadata.

Current data support:

- embedded project defaults are loaded;
- compact GoArrow `<loc>` records are supported;
- current port `<Location>` records are supported;
- Warcry Atlas `<atlas><location>` records can be parsed;
- route finding excludes retired or `UseInRouteFinding == false` locations.

Implemented for the first data-provider increment:

- explicit `/go update` command;
- configurable HTTP/HTTPS download URL;
- `/go url <url>` to persist a new source URL;
- `/go update <url>` to change the URL and immediately download;
- `/go file filename.xml` to load a local XML file from the plugin's hardcoded `GoArrow/` storage directory through `IPluginStorage.ReadText`;
- HTTP download from the configured source URL;
- response status, size, XML-root, and non-empty-record validation;
- atomic-after-validation cache write under `data/warcry-atlas.xml`;
- cached-data fallback when a later download fails;
- no automatic download during plugin startup;
- cancellation support in the provider API.

Still deferred:

- cache expiry and user-configurable refresh policy;
- panel UI for editing the data URL;
- merge precedence between embedded, installed, and user data;
- license/attribution workflow for external data;
- full import of every Atlas field such as restrictions, settlement, monsters, and time-of-day requirements;
- UI progress and cancellation controls.

The downloader remains opt-in and separate from the parser. `/go update` replaces the location snapshot only after the downloaded XML has been validated.

### 3. Route finding

The port now uses a weighted location graph and A* shortest-path search as the primary route-finding algorithm. `RouteGraph` builds eligible location nodes, connects nearby nodes with bidirectional walk edges, adds route-start edges, and converts paths into the existing `Route`/`RouteStep` model. `RouteFinder` adds the initial walk from the player's current position to the nearest graph node and falls back to a direct walk when the graph cannot produce a route.

Implemented:

- graph construction filtered by `UseInRouteFinding`, `IsRetired`, and coordinates;
- configurable maximum walk distance between graph nodes;
- deterministic A* shortest-path search;
- multi-hop walk routes;
- route-start edges classified as walk, portal, recall, or lifestone;
- route conversion into travel, portal, and recall steps;
- explicit pause/resume for portal and recall steps when no interaction API is available;
- unreachable, retired, duplicate-name, and edge-case tests;
- direct-walk fallback for destinations outside the graph.

Missing or incomplete behavior:

- explicit portal-device entrance and exit edges when records provide `Entrance`/`From` and `Exit`/`To` locations;
- lifestone bind and lifestone tie state;
- primary and secondary portal tie state;
- house and mansion recall state;
- allegiance bindstone state;
- portal-device usage requirements and interaction actions;
- arrival/exit coordinates on portal and dungeon transitions;
- graph invalidation/rebuild after location data updates;
- route cost profiles and configurable edge priorities;
- multiple candidate routes and detailed route explanations.

Next work:

1. Add supported portal, door, NPC, and recall interaction APIs.
2. Make graph snapshots rebuild atomically when `/go update` or `/go file` replaces data.
3. Add route cost policies for walking, recalls, portals, and unavailable actions.
4. Expose route steps and the active leg in the panel.
5. Integrate navigation reports so multi-leg routes advance reliably.
6. Add route tests for portal transitions, alternate routes, and data reloads.

### 4. Commands

Currently implemented commands:

- `/go help`;
- `/go list`;
- `/go search <term>` / `/go find <term>`;
- `/go loc`;
- `/go dest`;
- `/go file filename.xml`;
- `/go update` / `/go download`;
- `/go url <url>`;
- `/go status`;
- `/go route` / `/go steps`;
- `/go stop` / `/go cancel`;
- `/go clear`;
- `/go save <name>`;
- `/go favorites` / `/go favs`;
- `/go recall` placeholder behavior.

Still needed for original command parity:

- `/go to <coords|here|location>`;
- `/go from` / `/go start`;
- `/go end`;
- `/go loc`;
- `/go dest`;
- `/go find` / `/go search`;
- `/go reset`;
- `/go lock` / `/go unlock`;
- object attach/tag commands;
- aliases and quoted multi-word arguments;
- command completion.

The command prefix remains `/go`; changing it to `@go` is not required by the OpenAC contract.

### 5. Recall and bind tracking

Current state:

- settings fields exist for recall names;
- no authoritative recall-state provider is currently wired;
- no complete chat heuristic tracker is implemented;
- no raw house/allegiance network tracking is planned.

Required future OpenAC support:

- normalized primary/secondary portal recall state;
- house recall state;
- allegiance recall state;
- successful cast/use/portal transition reports;
- character and world scoping;
- stale/unknown/unavailable status.

Until those APIs exist, recall values should be manual or explicitly marked as inferred. They must not be presented as authoritative.

### 6. Navigation

Current state:

- `GoArrowNavigator` submits `GoTo` point requests for travel legs;
- it consumes `NavigationChanged` and filters stale/duplicate reports;
- it advances route steps on matching arrival states;
- it pauses at portal and recall legs and supports manual `/go resume` continuation;
- it stops on completion, failure, or manual cancellation;
- it handles unavailable/headless automation without throwing.

Needs improvement:

- request ownership and sequence validation;
- event-driven `GoToReport` updates;
- robust handling of `NoRoute`, `Blocked`, `Interrupted`, `Lost`, and `Waiting`;
- portal-space transition recovery;
- route-leg IDs and stale-report rejection;
- object-target navigation;
- interaction steps for portals, doors, and NPCs;
- explicit acceptance/rejection handling from `GoTo`.

### 7. Panel/UI

Current panel displays:

- destination;
- distance;
- bearing;
- navigation status;
- route-step count;
- start/stop/clear actions;
- basic display and recalculation toggles.

Still needed:

- destination text input;
- location autocomplete;
- route-step list;
- selected/current leg display;
- search results;
- route profile editor;
- validation/error display;
- reliable binding invalidation on each tick;
- panel focus and visibility commands.

### 8. HUDs and maps

Deferred because OpenAC currently has no plugin-owned transparent rendering surface or general map/canvas control.

Required OpenAC capabilities are documented in `docs/OpenAC-improvements.md`, including:

- plugin-owned HUD registration;
- host-managed textures and images;
- canvas/map controls;
- map coordinate conversion;
- markers, route lines, zoom, pan, and selection;
- persisted HUD position, size, scale, and visibility.

### 9. External route-data provider

This is deliberately later-phase work.

Before adding an HTTP provider, implement:

1. full location metadata model — now substantially restored;
2. typed data-provider interface;
3. schema/version validation;
4. explicit opt-in update action;
5. local cache and atomic replacement;
6. timeout, cancellation, retry, and offline behavior;
7. coordinate and type conversion tests;
8. retired/invalid record policy;
9. duplicate-name and ID conflict policy;
10. copyright and attribution review.

Suggested future interface:

```csharp
public interface ILocationDataProvider
{
    string Id { get; }
    Task<LocationDataSnapshot> LoadAsync(
        LocationDataLoadOptions options,
        CancellationToken cancellationToken);
}
```

The provider should return a complete immutable snapshot. `LocationDatabase` should atomically replace its data rather than mutate collections while route-finding is running.

## OpenAC API mapping

| Need | Existing API | Assessment |
| --- | --- | --- |
| Lifecycle | `IAcDreamPlugin` | Sufficient |
| Storage | `IPluginStorage` | Sufficient for current settings; scoped/structured storage would improve it |
| Commands | `IPluginCommandRegistry` | Sufficient for basic `/go`; completion/quoting would improve parity |
| Chat output | `IAutomation.Chat.PostSystemMessage` | Sufficient for current status output |
| Chat links | No structured link/click API | Required for clickable coordinate parity |
| Position | `INavigationAutomation.Snapshot` | Sufficient for basic display; position events would improve reliability |
| Walking | `GoTo` / `StopGoTo` / `GoToReport` | Sufficient for basic point navigation; ownership/events/interactions needed for robust routes |
| World objects | navigation object lookup and OpenAC events | Partially sufficient; needs complete plugin wiring and semantic object capabilities |
| Panels | `IUiRegistry.AddPanel` | Sufficient for current panel; input/list/autocomplete controls needed for parity |
| Rendering | No plugin-owned rendering surface | Required for arrow/map/toolbar HUDs |
| Recall state | No complete normalized recall API | Required for automatic bind/recall tracking |
| External data | No plugin data-provider/download service | Can be implemented later in the plugin once storage/resource policy is defined |

## Implementation order

1. **Correctness and tests**
   - Finish destination-kind model and command parsing.
   - Add lifecycle, command, storage, and navigator tests.
   - Add route graph tests. **Done** — graph construction, A* routing, multi-hop paths, route-start edges, unreachable routes, and filtering are covered.

2. **Complete location/route domain**
   - Finish original location types and route metadata.
   - Add explicit portal and interaction edge types.
   - Add atomic graph rebuilds when location data is replaced.
   - Add custom/favorite/recent location persistence.

3. **Navigation reliability**
   - Add request ownership, sequence IDs, event-driven reports, and portal transition recovery.
   - Add object interaction actions.

4. **Recall/bind state**
   - Add semantic OpenAC recall and transition APIs.
   - Replace manual/chat-only values with authoritative state where available.

5. **Panel parity**
   - Add input, autocomplete, route list/editor, and binding refresh behavior.

6. **External data provider**
   - Add opt-in, cached, validated provider for Atlas data only after licensing and schema policy are settled.

7. **Rendering and maps**
   - Add OpenAC render/map APIs, then port arrow, toolbar, Dereth map, and dungeon map HUDs.

8. **Chat parity**
   - Add structured coordinate links and click actions.

## Completion criteria

The port is behaviorally complete when it can provide, using only supported OpenAC APIs:

- directional arrow HUD;
- Dereth and dungeon map HUDs;
- toolbar HUD;
- clickable coordinate/object links;
- full named-location and coordinate destination workflows;
- complete graph routing with recall, portal, and interaction edges;
- authoritative recall/house/allegiance tracking;
- robust multi-leg navigation and portal recovery;
- persistent per-character settings and route data;
- user-updatable external location data;
- equivalent command, panel, chat, map, and HUD workflows.

Until then, the repository should be described as a supported core navigation port rather than a complete replacement for the original GoArrow plugin.
