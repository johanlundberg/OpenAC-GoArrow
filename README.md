# OpenAC GoArrow

`openac.goarrow` is a port of the classic GoArrow navigation plugin to the OpenAC plugin model.

This project preserves the core GoArrow idea—finding and following routes to named locations in Dereth—while adapting the implementation to OpenAC's supported `AcDream.Plugin.Abstractions` contract.

It is **not an official Digero, Virindi, Kaldor Greybear, or OpenAC project**. It is an independent port intended for the OpenAC plugin ecosystem.

## Thanks and attribution

Many thanks to **Digero**, the original creator of GoArrow, for designing and developing the plugin that this project ports.

Many thanks to **Virindi** for the Virindi View System and the broader plugin ecosystem and documentation that supported the original GoArrow experience.

Many thanks to **Kaldor Greybear** for maintaining the `GoArrow-VVSEdition` fork and preserving a usable reference implementation, including its route-finding and map-data work. That repository was used as the primary source reference for this port:

- Original reference repository: <https://github.com/kaldorgreybear/AsheronsCall-VTGoArrow>
- Virindi GoArrow documentation: <http://virindi.net/wiki/index.php/GoArrow_(VVS_Edition)>

The original reference repository is distributed under the MIT License. See the upstream [GoArrow-VVSEdition license](https://github.com/kaldorgreybear/AsheronsCall-VTGoArrow/blob/master/LICENSE) for the original license text. Attribution to Digero, the original GoArrow creator, and to Kaldor Greybear, the VVSEdition maintainer, should be preserved in redistributed versions.

## What is ported

The current port includes:

- named-location data structures;
- NS/EW coordinate parsing and distance/bearing calculations;
- location, portal-device, and route-start databases;
- route construction and route-finding logic;
- embedded default location and portal data;
- explicit `/go update` download of the configured location database with validation and cache fallback;
- loading local XML from the plugin's `GoArrow/` storage directory with `/go file filename.xml`;
- persistent URL switching through `/go url <url>` or `/go update <url>`;
- `/go` chat commands;
- persistent settings through `IPluginStorage`;
- a declarative OpenAC panel;
- plugin canvases for a directional arrow and compact Stop/Resume toolbar;
- a Dereth map surface with route and position markers, plus a background when the host supplies the map resource;
- coordinate chat links that set a destination;
- destination distance, bearing, and route status display;
- optional integration with `INavigationAutomation.GoTo`;
- graceful behavior when running in a headless host or outside a live world session;
- unit tests for the route-finding layer.

## OpenAC adaptations

The original plugin was built for Virindi/Decal and could use client-specific services that are not part of OpenAC's supported plugin contract. This port therefore makes several deliberate substitutions:

| Original GoArrow capability | OpenAC port | Current status |
| --- | --- | --- |
| Directional D3D arrow HUD | Plugin canvas arrow pointing to the next route waypoint, plus a declarative panel | Adapted; original arrow artwork is not reproduced |
| Dereth and dungeon map HUDs | Dereth map surface with route and position markers | Dereth map adapted; dungeon maps are not implemented |
| Floating toolbar HUD | Compact Stop/Resume canvas, panel buttons, and `/go` commands | Partially adapted |
| Clickable coordinate chat links | Coordinate-link handler sets the destination | Adapted through OpenAC's chat API |
| Raw client/network tracking | Supported snapshots and chat/state heuristics | Limited by available abstractions |
| Manual movement/key control | `INavigationAutomation.GoTo` | Adapted to OpenAC navigation |
| Decal XML/data services | `System.Xml` and embedded resources | Adapted |
| Virindi settings/profile storage | `IPluginStorage` | Adapted |

The historical improvement backlog is in [`OpenAC-improvements.md`](docs/OpenAC-improvements.md). Some APIs proposed there have since been added to OpenAC.

## Project layout

```text
src/AcDream.Plugins.GoArrow/
├── GoArrowPlugin.cs
├── GoArrowPanel.cs
├── GoArrowCommands.cs
├── GoArrowDestination.cs
├── GoArrowNavigator.cs
├── GoArrowSettings.cs
├── GoArrowHud.cs
├── GoArrowMap.cs
├── goarrow-panel.xml
├── plugin.json
├── RouteFinding/
│   ├── Coordinates.cs
│   ├── Location.cs
│   ├── LocationDatabase.cs
│   ├── PortalDevice.cs
│   ├── Route.cs
│   ├── RouteFinder.cs
│   └── RouteStart.cs
└── EmbeddedResources/
    ├── DefaultLocations.xml
    ├── DefaultPortalDevices.xml
    └── DefaultRouteStarts.xml
```

[`PORTING_PLAN.md`](docs/PORTING_PLAN.md) contains the original feature-by-feature porting plan. [`REFERENCES.md`](docs/REFERENCES.md) lists the primary source references.

## Building

The plugin targets .NET 10 and references the OpenAC abstractions project. With the OpenAC repository checked out beside this repository as `../OpenAC`, build the plugin with:

```bash
dotnet build src/AcDream.Plugins.GoArrow/AcDream.Plugins.GoArrow.csproj
```

The project currently builds cleanly with the supported OpenAC abstractions.

The test project is located at:

```text
tests/AcDream.Plugins.GoArrow.Tests/
```

Run it with:

```bash
dotnet test tests/AcDream.Plugins.GoArrow.Tests/AcDream.Plugins.GoArrow.Tests.csproj
```

The route-finding tests are intentionally independent of a live Asheron's Call session.

## CI and releases

GitHub Actions runs the plugin build and test suite for pull requests and pushes. A semantic version tag creates a GitHub release containing an installable plugin archive:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Release tags must use the `vMAJOR.MINOR.PATCH` format. The release version is derived from the tag (`v0.1.0` produces plugin version `0.1.0`), and the generated `plugin.json` version is updated automatically.

## Installation

OpenAC plugin packaging and installation conventions may vary by host version. In general, build or publish the plugin into its own plugin directory and deploy the following files together:

- `AcDream.Plugins.GoArrow.dll`;
- `plugin.json`;
- `goarrow-panel.xml`;
- any generated dependency files required by the OpenAC host.

The embedded route resources are compiled into the plugin assembly. The local `reference_repos` directory is development-only and intentionally ignored by Git; do not copy it into an installed plugin directory.

## Basic usage

After the plugin is enabled:

```text
/go help
/go list
/go file filename.xml
/go update
/go url https://example.test/locations.xml
/go update https://example.test/locations.xml
/go Holtburg
/go status
/go stop
/go clear
```

The destination name must exist in the loaded location database. With Auto-Navigate off, **Go** computes and displays a scrollable route list without moving the character. The arrow points to the first waypoint and updates relative to the character's heading. With Auto-Navigate on, Go also starts walking the route. The panel displays the destination, estimated distance, bearing, route status, and steps.

Navigation automation is optional. When no live session or navigation provider is available, GoArrow remains usable for route and destination information but cannot walk the character.

## Scope and limitations

This repository is a porting project, not a claim that every original GoArrow feature is already reproduced. In particular, the current port does not yet provide:

- the original arrow artwork and tooltip HUDs;
- dungeon and floor maps;
- every action from the original toolbar;
- authoritative recall, house, and allegiance tracking;
- complete portal/NPC/door interaction automation;
- all original data files and map assets;
- every original Virindi settings/profile workflow.

These features remain outside the current port. Extensions should use OpenAC's public abstractions instead of references to client internals, raw network messages, or client memory access.

## Contributing

When extending this port:

1. Keep plugin code dependent only on `AcDream.Plugin.Abstractions` and normal .NET libraries.
2. Preserve headless/inert-host behavior.
3. Add or update route-finding tests for domain changes.
4. Add host-level tests when using new OpenAC abstractions.
5. Keep original GoArrow behavior and attribution documented when porting additional code or data.
6. Prefer semantic OpenAC APIs over raw protocol or renderer-specific shortcuts.

For proposed OpenAC API changes, use the historical [`OpenAC-improvements.md`](docs/OpenAC-improvements.md) backlog as a starting point and check which APIs are already available.

## License and provenance

The original GoArrow-VVSEdition reference repository is MIT-licensed; its license is retained in the checked-out reference repository. New port code in this repository should have its licensing and attribution clarified before redistribution, especially when copying substantial source code or data from the original project.

This project is provided as an OpenAC porting effort and is offered without warranties. Asheron's Call, GoArrow, Digero, Virindi, Kaldor Greybear, and related names belong to their respective owners and contributors.
