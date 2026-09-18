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
- `/go` chat commands;
- persistent settings through `IPluginStorage`;
- a declarative OpenAC panel;
- destination distance, bearing, and route status display;
- optional integration with `INavigationAutomation.GoTo`;
- graceful behavior when running in a headless host or outside a live world session;
- unit tests for the route-finding layer.

## OpenAC adaptations

The original plugin was built for Virindi/Decal and could use client-specific services that are not part of OpenAC's supported plugin contract. This port therefore makes several deliberate substitutions:

| Original GoArrow capability | OpenAC port | Current status |
| --- | --- | --- |
| Directional D3D arrow HUD | Declarative panel with bearing and distance | Rendering HUD deferred until OpenAC exposes a supported rendering surface |
| Dereth and dungeon map HUDs | Embedded route data and panel status | Map canvas support not currently available |
| Floating toolbar HUD | Panel buttons and `/go` commands | Adapted |
| Clickable coordinate chat links | Manual `/go` commands | Chat link API not currently available |
| Raw client/network tracking | Supported snapshots and chat/state heuristics | Limited by available abstractions |
| Manual movement/key control | `INavigationAutomation.GoTo` | Adapted to OpenAC navigation |
| Decal XML/data services | `System.Xml` and embedded resources | Adapted |
| Virindi settings/profile storage | `IPluginStorage` | Adapted |

The detailed improvement backlog is in [`OpenAC-improvements.md`](OpenAC-improvements.md). It describes the OpenAC capabilities that would be needed for a fully feature-equivalent port.

## Project layout

```text
src/AcDream.Plugins.GoArrow/
├── GoArrowPlugin.cs
├── GoArrowPanel.cs
├── GoArrowCommands.cs
├── GoArrowDestination.cs
├── GoArrowNavigator.cs
├── GoArrowSettings.cs
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

`PORTING_PLAN.md` contains the original feature-by-feature porting plan. `REFERENCES.md` lists the primary source references.

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
/go Holtburg
/go status
/go stop
/go clear
```

The destination name must exist in the loaded location database. The panel displays the selected destination, estimated distance, bearing, route status, and route-step count.

Navigation automation is optional. When no live session or navigation provider is available, GoArrow remains usable for route and destination information but cannot walk the character.

## Scope and limitations

This repository is a porting project, not a claim that every original GoArrow feature is already reproduced. In particular, the current port does not yet provide:

- custom arrow rendering;
- Dereth or dungeon map windows;
- the original toolbar and tooltip HUDs;
- clickable coordinates in chat;
- authoritative recall, house, and allegiance tracking;
- complete portal/NPC/door interaction automation;
- all original data files and map assets;
- every original Virindi settings/profile workflow.

These limitations are intentional where OpenAC does not currently expose an equivalent supported API. They should not be worked around with references to OpenAC internals, raw network messages, or client memory access. Improvements should be made in OpenAC's public abstractions first, then consumed by this plugin.

## Contributing

When extending this port:

1. Keep plugin code dependent only on `AcDream.Plugin.Abstractions` and normal .NET libraries.
2. Preserve headless/inert-host behavior.
3. Add or update route-finding tests for domain changes.
4. Add host-level tests when using new OpenAC abstractions.
5. Keep original GoArrow behavior and attribution documented when porting additional code or data.
6. Prefer semantic OpenAC APIs over raw protocol or renderer-specific shortcuts.

For proposed OpenAC API changes, use [`OpenAC-improvements.md`](OpenAC-improvements.md) as the starting backlog.

## License and provenance

The original GoArrow-VVSEdition reference repository is MIT-licensed; its license is retained in the checked-out reference repository. New port code in this repository should have its licensing and attribution clarified before redistribution, especially when copying substantial source code or data from the original project.

This project is provided as an OpenAC porting effort and is offered without warranties. Asheron's Call, GoArrow, Digero, Virindi, Kaldor Greybear, and related names belong to their respective owners and contributors.
