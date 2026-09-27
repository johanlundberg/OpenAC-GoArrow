using System.Text.Json;
using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow;

// Directed edges represent observed movement, never inferred connectivity.
// Heights and cell IDs keep overlapping dungeon floors and rooms separate.
internal sealed class DungeonTraversalStore(IPluginStorage storage)
{
    internal const string StorageKey = "GoArrow/dungeon-traversals.json";
    private Data _data = new();
    private int? _previous;
    private bool _dirty;
    public string Error { get; private set; } = "";

    public sealed record Point(uint CellId, double EastWest, double NorthSouth, double Elevation)
    {
        internal PluginNavigationPosition Position => new(CellId, EastWest, NorthSouth, Elevation, 0, false);
    }
    public sealed record Edge(int From, int To);
    public sealed record Exit(uint Landblock, string Key, string Name, string Destination, int Anchor);
    public sealed class Data
    {
        public int Version { get; set; } = 1;
        public List<Point> Points { get; set; } = [];
        public List<Edge> Edges { get; set; } = [];
        public List<Exit> Exits { get; set; } = [];
    }

    public void Load()
    {
        try
        {
            if (storage.ReadText(StorageKey) is { } json)
                _data = Parse(json);
        }
        catch (Exception e) when (e is JsonException or ArgumentException or IOException or UnauthorizedAccessException)
        { Error = $"Dungeon traversal data could not be loaded: {e.Message}"; }
    }

    public void BreakTrace() => _previous = null;

    public void Observe(PluginNavigationSnapshot snapshot)
    {
        var p = snapshot.Position;
        if (!snapshot.IsAvailable || snapshot.IsPortalSpace || p.IsOutdoor || !Valid(p))
        { BreakTrace(); return; }
        if (_previous is int previous)
        {
            var last = _data.Points[previous].Position;
            if ((last.CellId & 0xFFFF0000u) != (p.CellId & 0xFFFF0000u) || Distance(last, p) > 12)
                BreakTrace(); // Teleports and large gaps do not establish a walkable edge.
            else if (last.CellId == p.CellId && Distance(last, p) < 3)
                return;
        }
        int next = FindOrAdd(p);
        if (_previous is int from && from != next && !_data.Edges.Contains(new Edge(from, next)))
        { _data.Edges.Add(new(from, next)); _dirty = true; }
        _previous = next;
    }

    public void RememberExit(string key, PluginWorldObject portal, PluginNavigationSnapshot snapshot)
    {
        if (string.IsNullOrWhiteSpace(portal.PortalDestination) || !Valid(snapshot.Position)
            || snapshot.Position.IsOutdoor || snapshot.IsPortalSpace)
            return;
        Observe(snapshot);
        if (_previous is not int anchor)
            return;
        var exit = new Exit(snapshot.Position.CellId & 0xFFFF0000u, key, portal.Name,
            portal.PortalDestination, anchor);
        if (!_data.Exits.Contains(exit))
        { _data.Exits.Add(exit); _dirty = true; }
        Save();
    }

    public IReadOnlyList<PluginNavigationPosition> FindPath(PluginNavigationPosition start, string key)
    {
        if (!Valid(start)) return [];
        uint block = start.CellId & 0xFFFF0000u;
        var goals = _data.Exits.Where(e => e.Landblock == block && e.Key == key).Select(e => e.Anchor).ToHashSet();
        if (goals.Count == 0) return [];
        // Join at a recorded point in the same cell, avoiding shortcuts
        // across walls or between vertically overlapping rooms.
        int first = Enumerable.Range(0, _data.Points.Count)
            .Where(i => _data.Points[i].CellId == start.CellId && Distance(start, _data.Points[i].Position) <= 8)
            .OrderBy(i => Distance(start, _data.Points[i].Position)).DefaultIfEmpty(-1).First();
        if (first < 0) return [];
        var adjacency = _data.Edges.ToLookup(e => e.From);
        var costs = new Dictionary<int, double> { [first] = 0 };
        var parents = new Dictionary<int, int>();
        var queue = new PriorityQueue<int, double>();
        queue.Enqueue(first, 0);
        while (queue.TryDequeue(out int node, out double cost))
        {
            if (cost > costs[node]) continue;
            if (goals.Contains(node))
            {
                var path = new List<PluginNavigationPosition>();
                for (int at = node;; at = parents[at])
                {
                    path.Add(_data.Points[at].Position);
                    if (at == first) break;
                }
                path.Reverse();
                return path;
            }
            foreach (var edge in adjacency[node])
            {
                double nextCost = cost + Distance(_data.Points[node].Position, _data.Points[edge.To].Position);
                if (costs.TryGetValue(edge.To, out double old) && old <= nextCost) continue;
                costs[edge.To] = nextCost;
                parents[edge.To] = node;
                queue.Enqueue(edge.To, nextCost);
            }
        }
        return [];
    }

    private int FindOrAdd(PluginNavigationPosition p)
    {
        for (int i = 0; i < _data.Points.Count; i++)
            if (_data.Points[i].CellId == p.CellId && Distance(_data.Points[i].Position, p) <= 1.5)
                return i;
        _data.Points.Add(new(p.CellId, p.EastWest, p.NorthSouth, p.Elevation));
        _dirty = true;
        return _data.Points.Count - 1;
    }

    public bool Save()
    {
        if (!_dirty) return true;
        if (!storage.IsAvailable) return false;
        try { storage.WriteText(StorageKey, Export()); _dirty = false; Error = ""; return true; }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or NotSupportedException)
        { Error = $"Dungeon traversal data could not be saved: {e.Message}"; return false; }
    }

    public string Export() => JsonSerializer.Serialize(_data, new JsonSerializerOptions { WriteIndented = true });

    public void Import(string json)
    {
        var incoming = Parse(json); // Validate the entire file before changing local records.
        var indices = incoming.Points.Select(p => FindOrAdd(p.Position)).ToArray();
        foreach (var e in incoming.Edges)
        {
            var mapped = new Edge(indices[e.From], indices[e.To]);
            if (mapped.From != mapped.To && !_data.Edges.Contains(mapped)) _data.Edges.Add(mapped);
        }
        foreach (var e in incoming.Exits)
        {
            var mapped = e with { Anchor = indices[e.Anchor] };
            if (!_data.Exits.Contains(mapped)) _data.Exits.Add(mapped);
        }
        _dirty = true;
        Save();
    }

    private static Data Parse(string json)
    {
        if (json.Length > 20_000_000) throw new ArgumentException("Traversal file is too large.");
        var data = JsonSerializer.Deserialize<Data>(json) ?? throw new ArgumentException("Empty traversal data.");
        if (data.Version != 1 || data.Points is null || data.Edges is null || data.Exits is null
            || data.Points.Count > 50_000 || data.Edges.Count > 100_000 || data.Exits.Count > 10_000)
            throw new ArgumentException("Unsupported or oversized traversal data.");
        bool Index(int i) => i >= 0 && i < data.Points.Count;
        if (data.Points.Any(p => p is null || !Valid(p.Position))
            || data.Edges.Any(e => e is null || !Index(e.From) || !Index(e.To)
                || (data.Points[e.From].CellId & 0xFFFF0000u) != (data.Points[e.To].CellId & 0xFFFF0000u)
                || Distance(data.Points[e.From].Position, data.Points[e.To].Position) > 12)
            || data.Exits.Any(e => e is null || !Index(e.Anchor) || string.IsNullOrWhiteSpace(e.Key)
                || string.IsNullOrWhiteSpace(e.Destination)
                || e.Landblock != (data.Points[e.Anchor].CellId & 0xFFFF0000u)))
            throw new ArgumentException("Invalid traversal points, edges, or exits.");
        return data;
    }

    private static bool Valid(PluginNavigationPosition p) => (p.CellId & 0xFFFF) >= 0x100
        && double.IsFinite(p.EastWest) && double.IsFinite(p.NorthSouth) && double.IsFinite(p.Elevation);
    private static double Distance(PluginNavigationPosition a, PluginNavigationPosition b) =>
        Math.Sqrt(Math.Pow(a.HorizontalDistanceMeters(b), 2) + Math.Pow((a.Elevation - b.Elevation) * 240, 2));
}
