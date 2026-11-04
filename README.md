# GoArrow

Navigation for the openAC-rust Asheron's Call client: a WebAssembly port of
GoArrow (Digero's Decal plugin, by way of Kaldor Greybear's VVS Edition and
johanlundberg's OpenAC port). Pick a place or map coordinates; GoArrow finds
a route over walks, portals and recalls, points the way, and can walk it.

```powershell
scripts/check.ps1   # format, lint, test, build target/wasm32-unknown-unknown/release/goarrow.wasm
```

Deliberate differences from OpenAC-GoArrow are in `DIVERGENCES.md`. MIT
(`LICENSE`); credits in `NOTICE.md`.

To install by hand, copy `plugin.json`, the built `goarrow.wasm` and the
`portals` folder into one folder under the client's plugins folder (the
window's markup is built into the module). `/go help` lists the commands.

GoArrow ships no places. When it starts with none cached it downloads them
(once, then from its cache) from the address in Options, or from
Darktorizo's GoArrow data when none is set; the setting
`DownloadDataAtStart` turns that off.

`portals/ace-portals.json` is the table of every portal in the world (where it
stands and where it leads), extracted from the ACE world database by
`tools/ace-portals.ps1`; it is AGPL-3.0 (`portals/LICENSE-AGPL-3.0.txt`), a
separate work read at run time (see `NOTICE.md`). GoArrow takes Atlas portal
arrivals from it where it has the portal. Without it GoArrow finds
dungeon exits by exploring and uses the Atlas arrivals, as before. (The
folder is not called `data`: the client keeps that name in a plugin's folder
for the player's own files, and the launcher refuses a package that ships
one.)
