<#
.SYNOPSIS
Extracts every portal in an ACE world database into portals/ace-portals.json.

.DESCRIPTION
Reads the portals the ACE server places in the world: every placed portal
instance, and the portals generators spawn where the spawn position follows
from the data (a profile with its own position, an offset from a generator
that stands at a known place, or the generator's own place). Random
scatters, creatures' spawns and spawns into containers are left out and
counted. House portals are another weenie type and are not read.

For each portal: its instance id, weenie class, name, cell, landblock-local
position (metres, to the centimetre) and heading, its destination (cell and
landblock-local position), its level limits and portal restrictions when it
has them, and whether it is walked into rather than used, seen only by
admins, linked to a generator, or spawned by one.

The table is a separate work under the AGPL-3.0, the licence of the ACE
world database; see portals/LICENSE-AGPL-3.0.txt and NOTICE.md.

The password is read only from the environment variable ACE_DB_PASSWORD and
handed to the MariaDB client in its own environment (MYSQL_PWD); it is
never written or printed.

.EXAMPLE
$env:ACE_DB_PASSWORD = '...'; tools/ace-portals.ps1
#>
[CmdletBinding()]
param(
    [string]$MySql = 'C:\Program Files\MariaDB 12.1\bin\mysql.exe',
    [string]$DbHost = '127.0.0.1',
    [int]$Port = 3306,
    [string]$User = 'root',
    [string]$Database = 'ace_world',
    [string]$Out = (Join-Path $PSScriptRoot '..\portals\ace-portals.json'),
    # The extraction date written into the header (default: today, UTC).
    [string]$Date = ([DateTime]::UtcNow.ToString('yyyy-MM-dd'))
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$inv = [Globalization.CultureInfo]::InvariantCulture

if (-not $env:ACE_DB_PASSWORD) {
    throw 'Set ACE_DB_PASSWORD to the ACE world database password.'
}
if (-not (Test-Path -LiteralPath $MySql)) {
    throw "No MariaDB client at '$MySql' (pass -MySql)."
}

# ---- the database ----

function Invoke-Query([string]$sql) {
    $savedPwd = $env:MYSQL_PWD
    $savedEncoding = [Console]::OutputEncoding
    try {
        $env:MYSQL_PWD = $env:ACE_DB_PASSWORD
        [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
        # The client warns that it does not check the server's certificate
        # when the password comes from the environment; other messages show.
        $lines = & $MySql --host=$DbHost --port=$Port --user=$User --database=$Database `
            --batch --skip-column-names --default-character-set=utf8mb4 --execute=$sql 2>&1 |
            ForEach-Object {
                if ($_ -is [Management.Automation.ErrorRecord]) {
                    if ("$_" -notmatch 'ssl-verify-server-cert') { Write-Warning "$_" }
                } else { $_ }
            }
        if ($LASTEXITCODE -ne 0) { throw "the query failed (exit $LASTEXITCODE)" }
    } finally {
        $env:MYSQL_PWD = $savedPwd
        [Console]::OutputEncoding = $savedEncoding
    }
    foreach ($line in @($lines)) {
        if ($null -eq $line -or $line -eq '') { continue }
        , ($line -split "`t" | ForEach-Object { Unescape $_ })
    }
}

# --batch writes NULL as NULL and escapes \, tab and newline.
function Unescape([string]$value) {
    if ($value -eq 'NULL') { return $null }
    if ($value.IndexOf('\') -lt 0) { return $value }
    $sb = [Text.StringBuilder]::new()
    for ($i = 0; $i -lt $value.Length; $i++) {
        $c = $value[$i]
        if ($c -eq '\' -and $i + 1 -lt $value.Length) {
            $i++
            switch ($value[$i]) {
                'n' { [void]$sb.Append("`n") }
                't' { [void]$sb.Append("`t") }
                '0' { [void]$sb.Append([char]0) }
                default { [void]$sb.Append($value[$i]) }
            }
        } else {
            [void]$sb.Append($c)
        }
    }
    $sb.ToString()
}

function Num($value) { if ($null -eq $value) { $null } else { [double]::Parse($value, $inv) } }
function Int($value) { if ($null -eq $value) { $null } else { [long]::Parse($value, $inv) } }

# ---- geometry ----

# A quaternion (w, x, y, z) turning a vector.
function Rotate($q, [double[]]$v) {
    $w, $x, $y, $z = $q
    # v + 2w(u x v) + 2(u x (u x v)), u = (x, y, z)
    $cx = $y * $v[2] - $z * $v[1]; $cy = $z * $v[0] - $x * $v[2]; $cz = $x * $v[1] - $y * $v[0]
    $ccx = $y * $cz - $z * $cy; $ccy = $z * $cx - $x * $cz; $ccz = $x * $cy - $y * $cx
    @(($v[0] + 2 * ($w * $cx + $ccx)), ($v[1] + 2 * ($w * $cy + $ccy)), ($v[2] + 2 * ($w * $cz + $ccz)))
}

# The product a * b of two quaternions (w, x, y, z).
function Multiply($a, $b) {
    $aw, $ax, $ay, $az = $a; $bw, $bx, $by, $bz = $b
    @(($aw * $bw - $ax * $bx - $ay * $by - $az * $bz),
      ($aw * $bx + $ax * $bw + $ay * $bz - $az * $by),
      ($aw * $by - $ax * $bz + $ay * $bw + $az * $bx),
      ($aw * $bz + $ax * $by - $ay * $bx + $az * $bw))
}

# The heading the client shows for an orientation: degrees clockwise from
# north (the turn about the vertical, counted from east towards north, as
# the client turns it into a heading).
function Heading($q) {
    $w, $x, $y, $z = $q
    $yaw = [Math]::Atan2(2 * ($w * $z + $x * $y), 1 - 2 * ($y * $y + $z * $z)) * 180 / [Math]::PI
    $h = (90 - $yaw) % 360
    if ($h -lt 0) { $h += 360 }
    $r = [int][Math]::Round($h, [MidpointRounding]::AwayFromZero)
    if ($r -eq 360) { 0 } else { $r }
}

function Cm([double]$v) {
    $r = [Math]::Round($v, 2, [MidpointRounding]::AwayFromZero)
    if ($r -eq 0) { $r = 0.0 }
    $r.ToString('0.##', $inv)
}

function Place([long]$cell, [double]$x, [double]$y, [double]$z, $q) {
    [pscustomobject]@{ Cell = $cell; X = $x; Y = $y; Z = $z; Q = $q }
}

# ---- reading ----

$version = @(Invoke-Query 'select base_Version, patch_Version, last_Modified from version order by id desc limit 1')
if ($version.Count -ne 1) { throw 'The database has no version row.' }
$version = $version[0]

# Portal weenies (type 7): name, destination, limits, use, visibility.
$weenies = @{}
foreach ($r in Invoke-Query @'
select w.class_Id, n.value,
  d.obj_Cell_Id, d.origin_X, d.origin_Y, d.origin_Z,
  (select value from weenie_properties_int where object_Id = w.class_Id and type = 86),
  (select value from weenie_properties_int where object_Id = w.class_Id and type = 87),
  (select value from weenie_properties_int where object_Id = w.class_Id and type = 111),
  (select value from weenie_properties_int where object_Id = w.class_Id and type = 16),
  (select value + 0 from weenie_properties_bool where object_Id = w.class_Id and type = 18)
from weenie w
left join weenie_properties_string n on n.object_Id = w.class_Id and n.type = 1
left join weenie_properties_position d on d.object_Id = w.class_Id and d.position_Type = 2
where w.type = 7
'@) {
    $weenies[(Int $r[0])] = [pscustomobject]@{
        Name = if ($null -eq $r[1]) { '' } else { $r[1] }
        Destination = if ($null -eq $r[2]) { $null } else { Place (Int $r[2]) (Num $r[3]) (Num $r[4]) (Num $r[5]) $null }
        MinLevel = Int $r[6]; MaxLevel = Int $r[7]; Bitmask = Int $r[8]
        # Usable.No: nothing to use; the portal is walked into.
        Walk = ((Int $r[9]) -eq 1)
        # Visibility: an object only admins see; the client is never sent it.
        Admin = ((Int $r[10]) -eq 1)
    }
}

# A portal linked to a "portaldestination" spot takes the spot as its
# destination (the last link wins).
$linkSpots = @{}
foreach ($r in Invoke-Query @'
select l.parent_GUID, c.obj_Cell_Id, c.origin_X, c.origin_Y, c.origin_Z
from landblock_instance_link l
join landblock_instance p on p.guid = l.parent_GUID
join weenie pw on pw.class_Id = p.weenie_Class_Id
join landblock_instance c on c.guid = l.child_GUID
join weenie cw on cw.class_Id = c.weenie_Class_Id
where pw.type = 7 and cw.type = 1 and cw.class_Name = 'portaldestination'
order by l.id
'@) {
    $linkSpots[(Int $r[0])] = Place (Int $r[1]) (Num $r[2]) (Num $r[3]) (Num $r[4]) $null
}

# Every generator profile, with its generator's weenie type, radius and
# generator type (1: relative).
$profiles = @(foreach ($r in Invoke-Query @'
select g.id, g.object_Id, gw.type, g.weenie_Class_Id, g.probability, g.where_Create,
  g.obj_Cell_Id, g.origin_X, g.origin_Y, g.origin_Z, g.angles_W, g.angles_X, g.angles_Y, g.angles_Z,
  (select value from weenie_properties_float where object_Id = g.object_Id and type = 43),
  (select value from weenie_properties_int where object_Id = g.object_Id and type = 100)
from weenie_properties_generator g
join weenie gw on gw.class_Id = g.object_Id
order by g.id
'@) {
    [pscustomobject]@{
        Id = Int $r[0]; Generator = Int $r[1]; GeneratorType = Int $r[2]; Spawn = Int $r[3]
        Probability = Num $r[4]; Where = Int $r[5]
        Cell = if ($null -eq $r[6]) { 0 } else { Int $r[6] }
        Offset = @(((Num $r[7]) ?? 0.0), ((Num $r[8]) ?? 0.0), ((Num $r[9]) ?? 0.0))
        Angles = @(((Num $r[10]) ?? 0.0), ((Num $r[11]) ?? 0.0), ((Num $r[12]) ?? 0.0), ((Num $r[13]) ?? 0.0))
        Radius = (Num $r[14]) ?? 0.0
        Relative = ((Int $r[15]) -eq 1)
    }
})
$profilesBySpawn = @{}
$generatorWeenies = @{}
foreach ($p in $profiles) {
    if (-not $profilesBySpawn.ContainsKey($p.Spawn)) { $profilesBySpawn[$p.Spawn] = [Collections.Generic.List[object]]::new() }
    $profilesBySpawn[$p.Spawn].Add($p)
    $generatorWeenies[$p.Generator] = $true
}

# Where generators stand: their placed instances.
$placedGenerators = @{}
foreach ($r in Invoke-Query @'
select i.weenie_Class_Id, i.obj_Cell_Id, i.origin_X, i.origin_Y, i.origin_Z,
  i.angles_W, i.angles_X, i.angles_Y, i.angles_Z
from landblock_instance i
where exists (select 1 from weenie_properties_generator g where g.object_Id = i.weenie_Class_Id)
order by i.guid
'@) {
    $w = Int $r[0]
    if (-not $placedGenerators.ContainsKey($w)) { $placedGenerators[$w] = [Collections.Generic.List[object]]::new() }
    $placedGenerators[$w].Add((Place (Int $r[1]) (Num $r[2]) (Num $r[3]) (Num $r[4]) @((Num $r[5]), (Num $r[6]), (Num $r[7]), (Num $r[8]))))
}

# Placed portal instances, with the instance a link makes each a child of
# and whether that parent is a generator.
$placed = @(Invoke-Query @'
select i.guid, i.weenie_Class_Id, i.obj_Cell_Id, i.origin_X, i.origin_Y, i.origin_Z,
  i.angles_W, i.angles_X, i.angles_Y, i.angles_Z,
  (select p.weenie_Class_Id from landblock_instance_link l join landblock_instance p on p.guid = l.parent_GUID
   where l.child_GUID = i.guid order by l.id limit 1)
from landblock_instance i
join weenie w on w.class_Id = i.weenie_Class_Id
where w.type = 7
order by i.guid
'@)

# ---- where a generator puts what it spawns ----

# The places a profile spawns at for one place of its generator, or a
# reason it has none fixed: as ACE places a spawn (a position of its own, an
# offset turned with the generator, a scatter of radius 0, or the
# generator's own place).
function Spawn-Place($p, $at) {
    if ($p.GeneratorType -eq 10) { return 'spawned by a creature (it moves)' }
    $where = $p.Where
    if ($where -band 4) {
        if ($p.Cell -gt 0) {
            $place = Place $p.Cell $p.Offset[0] $p.Offset[1] $p.Offset[2] $p.Angles
        } else {
            $o = Rotate $at.Q $p.Offset
            $q = if ($p.Relative) { Multiply $p.Angles $at.Q } else { $p.Angles }
            $place = Place $at.Cell ($at.X + $o[0]) ($at.Y + $o[1]) ($at.Z + $o[2]) $q
        }
        if (($place.Cell -shr 16) -ne ($at.Cell -shr 16)) { return 'specific place outside the generator''s landblock' }
        return $place
    }
    if ($where -band 2) {
        if ($p.Radius -gt 0) { return 'random scatter' }
        $x = $at.X; $y = $at.Y; $z = $at.Z
        if ($p.Cell -eq 0) { $x += $p.Offset[0]; $y += $p.Offset[1]; $z += $p.Offset[2] }
        return Place $at.Cell $x $y ($z + 0.05) $at.Q
    }
    if ($where -band (8 -bor 16 -bor 32)) { return 'not placed in the world (contained, wielded or sold)' }
    return Place $at.Cell $at.X $at.Y $at.Z $at.Q
}

# The fixed places a generator weenie stands at: placed, or spawned at a
# fixed place by a generator that has one (nested at most four deep).
$generatorPlaces = @{}
function Generator-Places([long]$w, [int]$depth) {
    if ($generatorPlaces.ContainsKey($w)) { return $generatorPlaces[$w] }
    $list = [Collections.Generic.List[object]]::new()
    if ($placedGenerators.ContainsKey($w)) { $list.AddRange($placedGenerators[$w]) }
    if ($depth -lt 4 -and $profilesBySpawn.ContainsKey($w)) {
        $generatorPlaces[$w] = $list # a loop reads what is known so far
        foreach ($p in $profilesBySpawn[$w]) {
            foreach ($at in (Generator-Places $p.Generator ($depth + 1))) {
                $place = Spawn-Place $p $at
                if ($place -isnot [string]) { $list.Add($place) }
            }
        }
    }
    $generatorPlaces[$w] = $list
    $list
}

# ---- the table ----

$entries = [Collections.Generic.List[object]]::new()
$counts = [ordered]@{ placed = 0; placedLinkedToGenerator = 0; generatorProfiles = 0; generatorProfilesIncluded = 0; generatorSpawned = 0; generatorSkipped = [ordered]@{}; withDestination = 0; withoutDestination = 0 }

function Skip([string]$why) {
    if (-not $counts.generatorSkipped.Contains($why)) { $counts.generatorSkipped[$why] = 0 }
    $counts.generatorSkipped[$why]++
}

foreach ($r in $placed) {
    $guid = Int $r[0]; $w = Int $r[1]
    $parent = Int $r[10]
    $source = if ($null -ne $parent -and $generatorWeenies.ContainsKey($parent)) { 'l' } else { $null }
    $destination = if ($linkSpots.ContainsKey($guid)) { $linkSpots[$guid] } else { $weenies[$w].Destination }
    $entries.Add([pscustomobject]@{
        Guid = $guid; Weenie = $w
        Place = Place (Int $r[2]) (Num $r[3]) (Num $r[4]) (Num $r[5]) @((Num $r[6]), (Num $r[7]), (Num $r[8]), (Num $r[9]))
        Destination = $destination; Source = $source; Generator = $null; Probability = $null
    })
    $counts.placed++
    if ($source) { $counts.placedLinkedToGenerator++ }
}

$seen = @{}
foreach ($p in $profiles) {
    if (-not $weenies.ContainsKey($p.Spawn)) { continue }
    $counts.generatorProfiles++
    if ($p.GeneratorType -eq 10) { Skip 'spawned by a creature (it moves)'; continue }
    $places = @(Generator-Places $p.Generator 0)
    if ($places.Count -eq 0) { Skip 'generator not placed at a known place'; continue }
    $made = 0; $why = $null
    foreach ($at in $places) {
        $place = Spawn-Place $p $at
        if ($place -is [string]) { $why = $place; continue }
        $key = '{0}|{1:X8}|{2}|{3}|{4}' -f $p.Spawn, $place.Cell, (Cm $place.X), (Cm $place.Y), (Cm $place.Z)
        if ($seen.ContainsKey($key)) { $made++; continue }
        $seen[$key] = $true
        $made++
        $entries.Add([pscustomobject]@{
            Guid = $null; Weenie = $p.Spawn; Place = $place
            Destination = $weenies[$p.Spawn].Destination; Source = 'g'; Generator = $p.Generator
            Probability = if ($p.Probability -gt 0 -and $p.Probability -lt 1) { $p.Probability } else { $null }
        })
        $counts.generatorSpawned++
    }
    if ($made -gt 0) { $counts.generatorProfilesIncluded++ } else { Skip $why }
}

foreach ($e in $entries) {
    if ($e.Destination) { $counts.withDestination++ } else { $counts.withoutDestination++ }
}

# ---- writing ----

function Json-String([string]$s) {
    $sb = [Text.StringBuilder]::new('"')
    foreach ($c in $s.ToCharArray()) {
        switch ($c) {
            '"' { [void]$sb.Append('\"') }
            '\' { [void]$sb.Append('\\') }
            default {
                if ([int]$c -lt 0x20) { [void]$sb.AppendFormat('\u{0:x4}', [int]$c) } else { [void]$sb.Append($c) }
            }
        }
    }
    [void]$sb.Append('"')
    $sb.ToString()
}

function Entry-Json($e) {
    $w = $weenies[$e.Weenie]
    $p = $e.Place
    $parts = [Collections.Generic.List[string]]::new()
    if ($null -ne $e.Guid) { $parts.Add('"g":' + $e.Guid) }
    $parts.Add('"w":' + $e.Weenie)
    $parts.Add('"n":' + (Json-String $w.Name))
    $parts.Add('"c":' + $p.Cell)
    $parts.Add('"p":[' + (Cm $p.X) + ',' + (Cm $p.Y) + ',' + (Cm $p.Z) + ']')
    if ($p.Q) { $parts.Add('"h":' + (Heading $p.Q)) }
    if ($e.Destination) {
        $d = $e.Destination
        $parts.Add('"d":' + $d.Cell)
        $parts.Add('"dp":[' + (Cm $d.X) + ',' + (Cm $d.Y) + ',' + (Cm $d.Z) + ']')
    }
    if ($null -ne $w.MinLevel) { $parts.Add('"lmin":' + $w.MinLevel) }
    if ($null -ne $w.MaxLevel) { $parts.Add('"lmax":' + $w.MaxLevel) }
    if ($null -ne $w.Bitmask) { $parts.Add('"pb":' + $w.Bitmask) }
    if ($w.Walk) { $parts.Add('"walk":1') }
    if ($w.Admin) { $parts.Add('"inv":1') }
    if ($e.Source) { $parts.Add('"src":"' + $e.Source + '"') }
    if ($null -ne $e.Generator) { $parts.Add('"gw":' + $e.Generator) }
    if ($null -ne $e.Probability) { $parts.Add('"pr":' + ([Math]::Round($e.Probability, 3)).ToString('0.###', $inv)) }
    '{' + ($parts -join ',') + '}'
}

$sorted = $entries | Sort-Object `
    @{ Expression = { $_.Place.Cell -shr 16 } }, `
    @{ Expression = { $_.Place.Cell } }, `
    @{ Expression = { if ($null -eq $_.Guid) { [long]::MaxValue } else { $_.Guid } } }, `
    @{ Expression = { $_.Weenie } }, `
    @{ Expression = { $_.Place.X } }, `
    @{ Expression = { $_.Place.Y } }, `
    @{ Expression = { $_.Place.Z } }

$skipped = ($counts.generatorSkipped.GetEnumerator() | ForEach-Object { (Json-String $_.Key) + ':' + $_.Value }) -join ','
$lines = [Collections.Generic.List[string]]::new()
$lines.Add('{')
$lines.Add('"format":1,')
$lines.Add('"source":"ACE world database (ACEmulator ACE-World-16PY with ACE-World-16PY-Patches), as installed on a local ACE server",')
$lines.Add('"version":{"base":' + (Json-String $version[0]) + ',"patch":' + (Json-String $version[1]) + ',"modified":' + (Json-String $version[2]) + '},')
$lines.Add('"extracted":' + (Json-String $Date) + ',')
$lines.Add('"license":"AGPL-3.0 (the ACE world database''s licence); see LICENSE-AGPL-3.0.txt. A separate work from GoArrow (MIT).",')
$lines.Add('"keys":"g instance guid; w weenie class; n name; c cell; p landblock-local x, y, z (m); h heading (degrees); d destination cell; dp its landblock-local x, y, z (m); lmin, lmax level limits; pb portal restrictions; walk walked into, not used; inv seen only by admins; src l linked to a generator, g spawned by generator gw; pr spawn probability",')
$lines.Add('"counts":{"placed":' + $counts.placed + ',"placedLinkedToGenerator":' + $counts.placedLinkedToGenerator +
    ',"generatorProfiles":' + $counts.generatorProfiles + ',"generatorProfilesIncluded":' + $counts.generatorProfilesIncluded +
    ',"generatorSpawned":' + $counts.generatorSpawned + ',"generatorProfilesSkipped":{' + $skipped + '}' +
    ',"withDestination":' + $counts.withDestination + ',"withoutDestination":' + $counts.withoutDestination + '},')
$lines.Add('"portals":[')
$all = @($sorted)
for ($i = 0; $i -lt $all.Count; $i++) {
    $lines.Add((Entry-Json $all[$i]) + $(if ($i -lt $all.Count - 1) { ',' } else { '' }))
}
$lines.Add(']}')

$outPath = [IO.Path]::GetFullPath($Out)
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($outPath)) | Out-Null
[IO.File]::WriteAllText($outPath, (($lines -join "`n") + "`n"), [Text.UTF8Encoding]::new($false))

$size = (Get-Item -LiteralPath $outPath).Length
Write-Output ('{0}: {1} portals ({2} placed, {3} generator-spawned from {4} of {5} profiles), {6} with a destination, {7:N0} bytes' -f `
    $outPath, $all.Count, $counts.placed, $counts.generatorSpawned, $counts.generatorProfilesIncluded, $counts.generatorProfiles, $counts.withDestination, $size)
foreach ($k in $counts.generatorSkipped.Keys) { Write-Output ('  skipped profiles: {0}: {1}' -f $k, $counts.generatorSkipped[$k]) }
