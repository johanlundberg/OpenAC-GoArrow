using AcDream.Plugin.Abstractions;
using System.Numerics;
using System.Diagnostics;

namespace AcDream.Plugins.GoArrow;

// Searches the known floorplan without treating any cell as a route arrival.
// All host calls and completed task reads happen on the plugin tick thread.
internal sealed class DungeonPortalSearch(DungeonTraversalStore? traversals = null)
{
    private string _exitKey = "";
    private bool _checkedRecordedPath;
    private bool _followingRecordedPath;
    private readonly Queue<PluginNavigationPosition> _recordedPath = [];
    private uint _landblock;
    private readonly HashSet<uint> _attempted = [];
    private readonly HashSet<uint> _covered = [];
    private readonly HashSet<uint> _shallowDeadEndCells = [];
    private PluginDungeonFloorplan? _recessPlan;
    private PluginDungeonFloorplan? _walkPlan;
    private readonly Dictionary<uint, List<(uint CellId, float Distance)>> _walkLinks = [];
    private readonly List<PluginNavigationPosition> _branch = [];
    private bool _returning;
    private string _lastFailure = string.Empty;
    private PluginNavigationPosition _target;
    private Task<PluginNavigationPlan>? _pending;
    private long _previewStarted;
    private PluginNavigationPosition? _lastPosition;
    public string Reason { get; private set; } = string.Empty;
    public IReadOnlyList<PluginNavigationPosition> RecordedWaypoints => _recordedPath.Take(80).ToArray();
    public int RemainingRecordedWaypoints => _recordedPath.Count;
    public PluginNavigationPosition CurrentTarget => _target;

    public void Reset()
    {
        _landblock = 0;
        _attempted.Clear();
        _covered.Clear();
        _shallowDeadEndCells.Clear();
        _recessPlan = null;
        _branch.Clear();
        _returning = false;
        _lastFailure = string.Empty;
        _pending = null;
        _lastPosition = null;
        _checkedRecordedPath = false;
        _followingRecordedPath = false;
        _recordedPath.Clear();
    }

    public void UseRecordedExit(string key)
    {
        if (_exitKey == key) return;
        Reset();
        _exitKey = key;
    }

    public bool TryGetNextTarget(IAutomationSurface automation, out PluginNavigationPosition target)
    {
        target = default;
        var snapshot = automation.Navigation.Snapshot;
        uint block = snapshot.Position.CellId & 0xFFFF0000u;
        if (!snapshot.IsAvailable || snapshot.IsPortalSpace || snapshot.Position.IsOutdoor)
            return false;
        if (_landblock != block)
        {
            Reset();
            _landblock = block;
        }
        if (_branch.Count == 0)
            _branch.Add(snapshot.Position);
        var portals = automation.Objects.CaptureObjects()
            .Where(p => p.HasPosition && (p.ObjectClass == PluginObjectClass.Portal
                || (p.Capabilities & PluginObjectCapabilities.Portal) != 0)
                && (p.Position.CellId & 0xFFFF0000u) == block).Select(p => p.Position).ToArray();
        if (_pending is not null)
        {
            if (!_pending.IsCompleted)
            {
                Reason = $"Checking portal clearance for cell {_target.CellId:X8} ({Stopwatch.GetElapsedTime(_previewStarted).TotalSeconds:0}s).";
                return false;
            }
            var completed = _pending;
            _pending = null;
            if (completed.IsCompletedSuccessfully && completed.Result.Status == PluginNavigationPlanStatus.Routed
                && completed.Result.Path.Count > 0 && !CrossesPortal(completed.Result.Path, portals))
            {
                target = _target;
                _lastPosition = snapshot.Position;
                SetTargetReason();
                return true;
            }
            if (_followingRecordedPath)
                CompleteTarget(snapshot.Position, false);
            else
                _attempted.Add(_target.CellId);
            _lastFailure = completed.IsCompletedSuccessfully ? completed.Result.Reason : "Portal clearance check failed.";
            if (_returning)
                CompleteTarget(snapshot.Position, false);
        }
        if (!_checkedRecordedPath)
        {
            _checkedRecordedPath = true;
            var trail = traversals?.FindPath(snapshot.Position, _exitKey) ?? [];
            var knownFloorplan = automation.DungeonMap.CaptureFloorplan(block);
            foreach (var point in SimplifyRecordedPath(trail, knownFloorplan, portals))
                _recordedPath.Enqueue(point);
        }
        while (_recordedPath.TryPeek(out var point)
            && snapshot.Position.HorizontalDistanceMeters(point) <= 2.5
            && Math.Abs(snapshot.Position.Elevation - point.Elevation) * 240 <= 2.5)
            _recordedPath.Dequeue();
        if (_recordedPath.TryPeek(out var recorded))
        {
            _followingRecordedPath = true;
            _target = recorded;
            return PrepareTarget(automation, snapshot, portals, out target);
        }
        var floorplan = automation.DungeonMap.CaptureFloorplan(block);
        UpdateShallowRecesses(floorplan);
        var cells = floorplan.Cells;
        var currentCell = cells.FirstOrDefault(c => c.CellId == snapshot.Position.CellId);
        double currentFloor = currentCell.CellId != 0 ? currentCell.LayerZ : snapshot.Position.Elevation * 240;
        MarkVisibleCells(snapshot.Position, floorplan);
        var floors = automation.DungeonMap.CaptureIndoorCells(block)
            .ToDictionary(c => c.CellId, c => c.Origin.Z);
        var candidates = cells.Where(c => IsOnFloor(c, floorplan))
            .Select(c => new Candidate(ToPosition(c, floors), c.LayerZ))
            .Where(c => !_attempted.Contains(c.Position.CellId)
                && !_covered.Contains(c.Position.CellId)
                && (!_shallowDeadEndCells.Contains(c.Position.CellId)
                    || portals.Any(portal => Math.Abs(portal.Elevation - c.Position.Elevation) * 240 <= 6
                        && portal.HorizontalDistanceMeters(c.Position) <= 36))
                && !_branch.Any(p => p.CellId == c.Position.CellId)
                && c.Position.CellId != snapshot.Position.CellId
                && (snapshot.Position.HorizontalDistanceMeters(c.Position) > 8
                    || Math.Abs(currentFloor - c.Height) > 3)
                && !portals.Any(portal => portal.HorizontalDistanceMeters(c.Position) < 12))
            .ToArray();
        // Distances follow the floorplan's open floor, so a corridor that
        // bends back still reads as leading on from here, not as a side
        // branch of the previous checkpoint.
        UpdateWalkGraph(floorplan);
        var fromHere = WalkDistances(snapshot.Position, floorplan);
        var fromPrevious = _branch.Count > 1 ? WalkDistances(_branch[^2], floorplan) : null;
        double Walk(IReadOnlyDictionary<uint, float> walks, PluginNavigationPosition from,
            PluginNavigationPosition to, IReadOnlyDictionary<uint, float>? other) =>
            walks.TryGetValue(to.CellId, out float walk) && (other is null || other.ContainsKey(to.CellId))
                ? walk : SearchDistance(from, to);
        // A target a shorter walk from here than from the previous
        // checkpoint continues this branch, even when it is farther than the usual nearby preference.
        // Return only after that direction has no unexplored targets.
        // Exits lie deeper far more often than higher, so a nearby way down
        // always continues the branch.
        var forward = _branch.Count > 1
            ? candidates.Where(c => IsBelow(c, currentFloor)
                    && snapshot.Position.HorizontalDistanceMeters(c.Position) <= NearbyMeters
                || Walk(fromHere, snapshot.Position, c.Position, fromPrevious) + 0.5
                    < Walk(fromPrevious!, _branch[^2], c.Position, fromHere)).ToArray()
            : candidates;
        _returning = forward.Length == 0 && _branch.Count > 1;
        if (_returning)
            _target = _branch[^2];
        else if (candidates.Length == 0)
        {
            Reason = cells.Count == 0 ? "Dungeon floorplan is unavailable."
                : $"No exploration targets remain ({_attempted.Count} attempted, {_covered.Count} covered). {_lastFailure}";
            return false;
        }
        else
        {
            var ranked = forward.Select(c => (c.Position, c.Height,
                    Walk: Walk(fromHere, snapshot.Position, c.Position, null)))
                .ToArray();
            var nearby = ranked.Where(c => c.Walk <= NearbyMeters).ToArray();
            // Lower storeys first, then the shortest walk; climbing costs
            // extra so a level route is taken over going back upstairs.
            _target = (nearby.Length > 0 ? nearby : ranked)
                .OrderBy(c => c.Height < currentFloor - 3 ? 0 : c.Height <= currentFloor + 3 ? 1 : 2)
                .ThenBy(c => c.Walk + Math.Max(0, c.Height - currentFloor) * 2)
                .ThenBy(c => c.Position.CellId).First().Position;
        }
        return PrepareTarget(automation, snapshot, portals, out target);
    }

    private const double NearbyMeters = 40;
    // Neighbouring cells further apart than this are joined through the
    // cells between them, which keeps each link short enough to trust.
    private const float WalkLinkMeters = 30;

    private readonly record struct Candidate(PluginNavigationPosition Position, float Height);

    private static bool IsBelow(Candidate candidate, double floor) => candidate.Height < floor - 3;

    private static double SearchDistance(PluginNavigationPosition from, PluginNavigationPosition to) =>
        from.HorizontalDistanceMeters(to) + Math.Abs(from.Elevation - to.Elevation) * 240;

    private bool PrepareTarget(IAutomationSurface automation, PluginNavigationSnapshot snapshot,
        IReadOnlyList<PluginNavigationPosition> portals, out PluginNavigationPosition target)
    {
        target = default;
        if (CrossesPortal([snapshot.Position, _target], portals))
        {
            _pending = automation.Navigation.PreviewPathAsync(_target, 2.5f);
            _previewStarted = Stopwatch.GetTimestamp();
            Reason = $"Checking portal clearance for cell {_target.CellId:X8}.";
            return false;
        }
        target = _target;
        _lastPosition = snapshot.Position;
        SetTargetReason();
        return true;
    }

    private void SetTargetReason() => Reason =
        $"{(_followingRecordedPath ? "Following recorded path to" : _returning ? "Backtracking to" : "Searching")} cell {_target.CellId:X8} "
        + $"({_attempted.Count} attempted, {_covered.Count} covered, {_branch.Count - 1} branch depth).";

    internal static IReadOnlyList<PluginNavigationPosition> SimplifyRecordedPath(
        IReadOnlyList<PluginNavigationPosition> path, PluginDungeonFloorplan plan,
        IReadOnlyList<PluginNavigationPosition> portals)
    {
        if (path.Count < 3) return path;
        var simplified = new List<PluginNavigationPosition> { path[0] };
        var layers = plan.Cells.ToDictionary(c => c.CellId, c => c.LayerZ);
        int from = 0;
        while (from < path.Count - 1)
        {
            int farthest = from + 1;
            layers.TryGetValue(path[from].CellId, out float fromLayer);
            for (int to = from + 2; to < Math.Min(path.Count, from + 64); to++)
            {
                if (!layers.TryGetValue(path[to - 1].CellId, out float middleLayer)
                    || middleLayer != fromLayer)
                    break;
                // A shortcut spans only a modest walk on one floor. The
                // host still plans each resulting segment against live walls.
                if (path[from].HorizontalDistanceMeters(path[to]) > 45)
                    continue;
                if (CanShortcut(path[from], path[to], plan, portals))
                    farthest = to;
            }
            simplified.Add(path[farthest]);
            from = farthest;
        }
        return simplified;
    }

    private static bool CanShortcut(PluginNavigationPosition start, PluginNavigationPosition end,
        PluginDungeonFloorplan plan, IReadOnlyList<PluginNavigationPosition> portals)
    {
        if (Math.Abs(start.Elevation - end.Elevation) * 240 > 3
            || CrossesPortal([start, end], portals))
            return false;
        var startCell = plan.Cells.FirstOrDefault(c => c.CellId == start.CellId);
        var endCell = plan.Cells.FirstOrDefault(c => c.CellId == end.CellId);
        if (startCell.CellId == 0 || endCell.CellId == 0
            || Math.Abs(startCell.LayerZ - endCell.LayerZ) > 0.1f)
            return false;
        var layer = plan.Layers.FirstOrDefault(l => Math.Abs(l.Z - startCell.LayerZ) < 0.1f);
        if (layer is null || layer.Floors.Count == 0)
            return false;
        var from = PluginDungeonFloorplan.ToLandblockLocal(start);
        var to = PluginDungeonFloorplan.ToLandblockLocal(end);
        var a = new Vector2(from.X, from.Y);
        var b = new Vector2(to.X, to.Y);
        if (layer.Walls.Any(w => IntersectsWall(a, b, w.Start, w.End)))
            return false;
        int samples = Math.Max(1, (int)Math.Ceiling(Vector2.Distance(a, b) / 2));
        for (int i = 0; i <= samples; i++)
        {
            var point = Vector2.Lerp(a, b, i / (float)samples);
            if (!layer.Floors.Any(polygon => Contains(polygon, point)))
                return false;
        }
        return true;
    }

    public void CompleteTarget(PluginNavigationPosition position, bool arrived)
    {
        if (_followingRecordedPath)
        {
            if (arrived && _recordedPath.Count > 0) _recordedPath.Dequeue();
            else _recordedPath.Clear(); // Fall back to exploration when a saved segment fails.
            _followingRecordedPath = false;
            return;
        }
        if (_returning)
        {
            if (arrived && _branch.Count > 1)
                _branch.RemoveAt(_branch.Count - 1);
            else
                _branch.Clear(); // An obstructed return cannot remain a usable branch.
        }
        else if (arrived && !_branch.Any(p => p.CellId == position.CellId))
            _branch.Add(position);
        _returning = false;
    }

    public void PauseTarget()
    {
        if (!_followingRecordedPath && !_returning)
            _attempted.Remove(_target.CellId);
        _returning = false;
    }

    public bool IsApproachingPortal(PluginNavigationPosition position, PluginNavigationPosition portal) =>
        _lastPosition is { } previous
        && position.HorizontalDistanceMeters(portal) + 0.05
            < previous.HorizontalDistanceMeters(portal);

    public bool SafeToContinue(IAutomationSurface automation, bool mayPassOutdoors = false)
    {
        var snapshot = automation.Navigation.Snapshot;
        if (!snapshot.IsAvailable || snapshot.IsPortalSpace
            || (snapshot.Position.IsOutdoor && !mayPassOutdoors))
            return false;
        var previous = _lastPosition;
        _lastPosition = snapshot.Position;
        if (previous is null)
            return true;
        foreach (var portal in automation.Objects.CaptureObjects())
        {
            if (!portal.HasPosition || (portal.Position.CellId & 0xFFFF0000u) != _landblock
                || (portal.ObjectClass != PluginObjectClass.Portal
                    && (portal.Capabilities & PluginObjectCapabilities.Portal) == 0))
                continue;
            double distance = snapshot.Position.HorizontalDistanceMeters(portal.Position);
            if (distance < 8 && distance + 0.05 < previous.Value.HorizontalDistanceMeters(portal.Position)
                && (!double.IsFinite(portal.Position.Elevation)
                    || Math.Abs(snapshot.Position.Elevation - portal.Position.Elevation) * 240 <= 6))
            {
                Reason = $"Stopped exploration before approaching portal '{portal.Name}'.";
                return false;
            }
        }
        return true;
    }

    internal static bool CrossesPortal(IReadOnlyList<PluginNavigationPosition> path,
        IReadOnlyList<PluginNavigationPosition> portals)
    {
        foreach (var portal in portals)
            for (int i = 1; i < path.Count; i++)
            {
                var start = path[i - 1];
                var end = path[i];
                double dx = end.EastWest - start.EastWest, dy = end.NorthSouth - start.NorthSouth;
                double length = dx * dx + dy * dy;
                double projection = (portal.EastWest - start.EastWest) * dx
                    + (portal.NorthSouth - start.NorthSouth) * dy;
                // Leaving a portal is allowed only when this segment heads
                // away from it; a far endpoint on the other side is unsafe.
                if (i == 1 && start.HorizontalDistanceMeters(portal) < 8 && projection <= 0)
                    continue;
                double t = length > 0 ? Math.Clamp(projection / length, 0, 1) : 0;
                double px = start.EastWest + t * dx - portal.EastWest;
                double py = start.NorthSouth + t * dy - portal.NorthSouth;
                double height = start.Elevation + t * (end.Elevation - start.Elevation);
                if ((px * px + py * py) * 240 * 240 < 64
                    && (!double.IsFinite(height) || !double.IsFinite(portal.Elevation)
                        || Math.Abs(height - portal.Elevation) * 240 <= 6))
                    return true;
            }
        return false;
    }

    public void TargetAccepted(uint cellId)
    {
        if (!_followingRecordedPath) _attempted.Add(cellId);
    }
    public void RecordFailure(string? reason) => _lastFailure = reason ?? "The last cell was unreachable.";

    private void MarkVisibleCells(PluginNavigationPosition position, PluginDungeonFloorplan plan)
    {
        if (plan.Layers.Count == 0)
            return;
        var currentCell = plan.Cells.FirstOrDefault(c => c.CellId == position.CellId);
        float z = currentCell.CellId != 0 ? currentCell.LayerZ : (float)(position.Elevation * 240);
        var layer = plan.Layers.OrderBy(l => Math.Abs(l.Z - z)).First();
        if (Math.Abs(layer.Z - z) > 6 || layer.Floors.Count == 0)
            return;
        var local = PluginDungeonFloorplan.ToLandblockLocal(position);
        var start = new Vector2(local.X, local.Y);
        foreach (var cell in plan.Cells)
        {
            var end = new Vector2(cell.Center.X, cell.Center.Y);
            // Nearby cells visible from here have already been observed; their
            // individual centres add no new search coverage. Walls and storeys
            // separate rooms, even when their centres are close together.
            if (Math.Abs(cell.LayerZ - layer.Z) < 0.1f
                && Vector2.Distance(start, end) <= 40
                && IsOnFloor(cell, plan)
                && !layer.Walls.Any(w => IntersectsWall(start, end, w.Start, w.End)))
                _covered.Add(cell.CellId);
        }
    }

    // Links cells that can see each other across open floor, and cells on
    // neighbouring storeys that stand nearly over each other, where stairs
    // and ramps join them. Hosts without walls get no graph, since every
    // straight line would look open.
    private void UpdateWalkGraph(PluginDungeonFloorplan plan)
    {
        if (ReferenceEquals(_walkPlan, plan))
            return;
        _walkPlan = plan;
        _walkLinks.Clear();
        if (plan.Layers.Count == 0)
            return;
        var cells = plan.Cells.Where(c => IsOnFloor(c, plan)).ToArray();
        foreach (var cell in cells)
            _walkLinks[cell.CellId] = [];
        for (int i = 0; i < cells.Length; i++)
            for (int j = i + 1; j < cells.Length; j++)
            {
                var a = cells[i];
                var b = cells[j];
                var start = new Vector2(a.Center.X, a.Center.Y);
                var end = new Vector2(b.Center.X, b.Center.Y);
                float across = Vector2.Distance(start, end);
                float rise = Math.Abs(a.LayerZ - b.LayerZ);
                float distance;
                if (rise < 0.1f)
                {
                    if (across > WalkLinkMeters
                        || plan.Layers.FirstOrDefault(l => Math.Abs(l.Z - a.LayerZ) < 0.1f) is not { } layer
                        || layer.Walls.Any(w => IntersectsWall(start, end, w.Start, w.End)))
                        continue;
                    distance = across;
                }
                else if (rise <= 6.5f && across <= 12)
                    distance = across + rise;
                else
                    continue;
                _walkLinks[a.CellId].Add((b.CellId, distance));
                _walkLinks[b.CellId].Add((a.CellId, distance));
            }
    }

    // Walking distance from a position to every cell the graph reaches.
    private Dictionary<uint, float> WalkDistances(PluginNavigationPosition position,
        PluginDungeonFloorplan plan)
    {
        var distances = new Dictionary<uint, float>();
        if (_walkLinks.Count == 0)
            return distances;
        var queue = new PriorityQueue<uint, float>();
        void Seed(uint cellId, float distance)
        {
            if (distances.TryGetValue(cellId, out float known) && known <= distance)
                return;
            distances[cellId] = distance;
            queue.Enqueue(cellId, distance);
        }
        if (_walkLinks.ContainsKey(position.CellId))
            Seed(position.CellId, 0);
        else
        {
            float z = (float)(position.Elevation * 240);
            var layer = plan.Layers.OrderBy(l => Math.Abs(l.Z - z)).First();
            var local = PluginDungeonFloorplan.ToLandblockLocal(position);
            var start = new Vector2(local.X, local.Y);
            foreach (var cell in plan.Cells)
            {
                var end = new Vector2(cell.Center.X, cell.Center.Y);
                float distance = Vector2.Distance(start, end);
                if (Math.Abs(cell.LayerZ - layer.Z) < 0.1f && distance <= WalkLinkMeters
                    && _walkLinks.ContainsKey(cell.CellId)
                    && !layer.Walls.Any(w => IntersectsWall(start, end, w.Start, w.End)))
                    Seed(cell.CellId, distance);
            }
        }
        while (queue.TryDequeue(out uint cellId, out float distance))
        {
            if (distance > distances[cellId])
                continue;
            foreach (var (next, step) in _walkLinks[cellId])
                Seed(next, distance + step);
        }
        return distances;
    }

    private void UpdateShallowRecesses(PluginDungeonFloorplan plan)
    {
        if (ReferenceEquals(_recessPlan, plan))
            return;
        _recessPlan = plan;
        _shallowDeadEndCells.Clear();
        foreach (var layer in plan.Layers)
        {
            if (layer.Floors.Count == 0 || layer.Walls.Count < 3)
                continue;
            var cells = plan.Cells.Where(c => Math.Abs(c.LayerZ - layer.Z) < 0.1f
                && IsOnFloor(c, plan)).ToArray();
            var neighbors = new Dictionary<uint, List<(uint CellId, float Distance)>>();
            foreach (var cell in cells)
            {
                var start = new Vector2(cell.Center.X, cell.Center.Y);
                var visible = cells.Where(other => other.CellId != cell.CellId)
                    .Select(other => (Cell: other, Delta: new Vector2(other.Center.X, other.Center.Y) - start))
                    .Where(p => p.Delta.Length() is >= 2 and <= 24
                        && !layer.Walls.Any(w => IntersectsWall(start, start + p.Delta, w.Start, w.End)))
                    .OrderBy(p => p.Delta.LengthSquared()).ToArray();
                var directions = new List<Vector2>();
                var links = new List<(uint CellId, float Distance)>();
                foreach (var (other, delta) in visible)
                {
                    var direction = Vector2.Normalize(delta);
                    // Several cell centres along one straight corridor are
                    // one way out, not separate branches.
                    if (directions.Any(known => Vector2.Dot(known, direction) > 0.8f))
                        continue;
                    directions.Add(direction);
                    links.Add((other.CellId, delta.Length()));
                }
                neighbors[cell.CellId] = links;
            }
            var byId = cells.ToDictionary(c => c.CellId);
            foreach (var leaf in cells)
            {
                if (neighbors[leaf.CellId].Count != 1
                    || !IsEnclosedRecess(leaf.Center, neighbors[leaf.CellId][0].Distance, layer.Walls))
                    continue;
                var branch = new List<uint>();
                uint previous = 0;
                uint current = leaf.CellId;
                float depth = 0;
                while (true)
                {
                    var links = neighbors[current];
                    var onward = links.Where(n => n.CellId != previous).ToArray();
                    float radius = links.Count == 0 ? 8 : links.Min(n => n.Distance);
                    if (!IsEnclosedRecess(byId[current].Center, radius, layer.Walls)
                        || onward.Length != 1)
                        break;
                    branch.Add(current);
                    depth += onward[0].Distance;
                    if (depth > 32 || branch.Count > 3)
                    {
                        branch.Clear();
                        break;
                    }
                    previous = current;
                    current = onward[0].CellId;
                    if (branch.Contains(current))
                    {
                        branch.Clear();
                        break;
                    }
                }
                if (branch.Count > 0 && depth <= 32 && current != leaf.CellId)
                    foreach (uint cellId in branch)
                        _shallowDeadEndCells.Add(cellId);
            }
        }
    }

    private static bool IsEnclosedRecess(Vector3 center, float entryDistance,
        IReadOnlyList<PluginDungeonWall> walls)
    {
        var start = new Vector2(center.X, center.Y);
        float radius = Math.Clamp(entryDistance, 6, 16);
        int blocked = 0;
        for (int direction = 0; direction < 8; direction++)
        {
            float angle = direction * MathF.PI / 4;
            var end = start + radius * new Vector2(MathF.Cos(angle), MathF.Sin(angle));
            if (walls.Any(w => IntersectsWall(start, end, w.Start, w.End)))
                blocked++;
        }
        return blocked >= 5;
    }

    private static bool IsOnFloor(PluginDungeonCell cell, PluginDungeonFloorplan plan)
    {
        var layer = plan.Layers.FirstOrDefault(l => Math.Abs(l.Z - cell.LayerZ) < 0.1f);
        // Hosts without floor polygons still use the reachability preview.
        if (layer is null)
            return true;
        var point = new Vector2(cell.Center.X, cell.Center.Y);
        return layer.Floors.Any(polygon => Contains(polygon, point));
    }

    private static bool Contains(IReadOnlyList<Vector2> polygon, Vector2 point)
    {
        bool inside = false;
        for (int i = 0, j = polygon.Count - 1; i < polygon.Count; j = i++)
        {
            var a = polygon[j];
            var b = polygon[i];
            var edge = b - a;
            if (edge.LengthSquared() > 0)
            {
                float t = Math.Clamp(Vector2.Dot(point - a, edge) / edge.LengthSquared(), 0, 1);
                if (Vector2.DistanceSquared(point, a + t * edge) < 0.0001f)
                    return true;
            }
            if ((a.Y > point.Y) != (b.Y > point.Y)
                && point.X < (b.X - a.X) * (point.Y - a.Y) / (b.Y - a.Y) + a.X)
                inside = !inside;
        }
        return inside;
    }

    private static bool IntersectsWall(Vector2 start, Vector2 end, Vector2 a, Vector2 b)
    {
        static float Cross(Vector2 x, Vector2 y) => x.X * y.Y - x.Y * y.X;
        var direction = end - start;
        var wall = b - a;
        float denominator = Cross(direction, wall);
        if (Math.Abs(denominator) < 0.0001f)
        {
            if (Math.Abs(Cross(a - start, direction)) > 0.0001f || direction.LengthSquared() < 0.0001f)
                return false;
            float first = Vector2.Dot(a - start, direction) / direction.LengthSquared();
            float last = Vector2.Dot(b - start, direction) / direction.LengthSquared();
            return Math.Max(first, last) > 0.001f && Math.Min(first, last) <= 1;
        }
        float t = Cross(a - start, wall) / denominator;
        float u = Cross(a - start, direction) / denominator;
        return t > 0.001f && t <= 1 && u >= 0 && u <= 1;
    }

    internal static PluginNavigationPosition ToPosition(PluginDungeonCell cell,
        IReadOnlyDictionary<uint, float> floors)
    {
        int x = (int)(cell.CellId >> 24);
        int y = (int)((cell.CellId >> 16) & 255);
        // Geometry centres may sit high above a room's floor, and leaving
        // height unknown makes the host resolve this point at terrain or
        // current height, which can turn a downstairs goal into an upstairs
        // walk. A cell's origin is where its piece's floor is built; the
        // layer only bounds the storey and can lie metres below the floor,
        // which the host cannot route to outside a sealed dungeon.
        float height = floors.TryGetValue(cell.CellId, out float origin)
            && Math.Abs(origin - cell.LayerZ) <= 6f ? origin : cell.LayerZ;
        return new PluginNavigationPosition(cell.CellId,
            ((x - 127) * 192 + cell.Center.X - 84) / 240d,
            ((y - 127) * 192 + cell.Center.Y - 84) / 240d,
            height / 240d, 0, false);
    }

}
