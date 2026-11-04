# Notice

GoArrow for openAC-rust is a port of **GoArrow**, the Asheron's Call
navigation plugin, to a WebAssembly plugin for openAC-rust.

- **Digero** created GoArrow, the original Decal plugin.
- **Virindi** created GoArrow (VVS Edition) and the Virindi View System it
  ran on.
- **Kaldor Greybear** maintains the GoArrow-VVSEdition reference
  implementation (<https://github.com/kaldorgreybear/AsheronsCall-VTGoArrow>,
  MIT), including its route finding and map data work.
- **johanlundberg** and the OpenAC-GoArrow contributors ported it to the C#
  OpenAC client (<https://github.com/johanlundberg/OpenAC-GoArrow>, MIT).
  This port follows that one (commit `fee38cb`): its commands, its data
  formats, its route graph and route search, and its navigation.

Both upstream projects are MIT-licensed, and OpenAC-GoArrow's licence asks
that attribution to Digero, Virindi and Kaldor Greybear be preserved for
portions derived from their work; `LICENSE` carries that clause and every
copyright line. Asheron's Call, GoArrow and the names above belong to their
respective owners. This is not an official project of any of them.

No Atlas location data is shipped; the one shipped route correction
(`crates/goarrow/data/atlas-corrections.json`, a portal arrival) is
OpenAC-GoArrow's. The Options tab downloads location data from a URL you
choose (for example Darktorizo's GoArrow Data,
<https://github.com/Darktorizo/GoArrow_Data_CoD>); that data is its
authors', under their terms.

## The portal table (`portals/ace-portals.json`)

`portals/ace-portals.json` lists the portals in the world: where each stands
(cell and position), where it leads, and its level limits and restrictions.
It is extracted by `tools/ace-portals.ps1` from the world database of the
ACE server emulator (ACEmulator's ACE-World-16PY with ACE-World-16PY-Patches,
<https://github.com/ACEmulator>); the file's header names the database
version and the extraction date.

That database is licensed under the GNU Affero General Public License,
version 3 (AGPL-3.0), and so is the table extracted from it; its text is
`portals/LICENSE-AGPL-3.0.txt`. The table is a separate work from GoArrow: it is
not compiled into the plugin, which reads it as a data file at run time and
runs without it. GoArrow's code stays under the MIT licence (`LICENSE`).

## The map icons (`crates/goarrow/assets/map/`)

The sixteen PNG files there are the original GoArrow's map icons (GoArrow
VVS Edition, `GoArrow.Icons.Map.resx`, by Digero, Virindi and Kaldor
Greybear, MIT), taken out of the resource file unchanged. The Dereth map's
zoom, grid, icons by zoom, names, route lines, hover and menus follow that
map's rules (`MapHud`); `LICENSE` carries the attribution its licence asks
for.

## Game data

The world maps (the game's small one, and the one the plugin draws from the
landscape's terrain), icons and dungeon floorplans the plugin shows are read
from the game's own files on your computer at run time.

The plugin is built on the openAC plugin SDK
(<https://github.com/eriknihlen/openac-rust>, MIT).
