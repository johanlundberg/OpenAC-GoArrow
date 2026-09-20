namespace AcDream.Plugins.GoArrow.RouteFinding;

/// <summary>
/// Edge kind in the route graph.
/// </summary>
public enum RouteEdgeKind
{
    Walk,
    Portal,
    Recall,
    Lifestone,
}

/// <summary>
/// A directed edge in the route graph, connecting two location nodes.
/// </summary>
public sealed class RouteGraphEdge
{
    /// <summary>Index of the source node.</summary>
    public int FromIndex { get; }

    /// <summary>Index of the destination node.</summary>
    public int ToIndex { get; }

    /// <summary>Type of traversal.</summary>
    public RouteEdgeKind Kind { get; }

    /// <summary>Travel cost (map units for walk, small constant for portal/recall).</summary>
    public double Cost { get; }

    /// <summary>Name of the portal device, recall spell, or method used.</summary>
    public string Via { get; }

    public RouteGraphEdge(int fromIndex, int toIndex, RouteEdgeKind kind, double cost, string via)
    {
        FromIndex = fromIndex;
        ToIndex = toIndex;
        Kind = kind;
        Cost = cost;
        Via = via ?? string.Empty;
    }

    public override string ToString() => $"[{Kind}] {FromIndex} → {ToIndex} ({Cost:F2} via {Via})";
}

/// <summary>
/// A weighted directed graph built from a <see cref="LocationDatabase"/>.
/// Supports A* shortest-path queries between named locations.
/// </summary>
public sealed class RouteGraph
{
    // ── Node storage ───────────────────────────────────────────────

    /// <summary>Node index → Location (indexes are stable after Build).</summary>
    private readonly List<Location> _locations = new();

    /// <summary>Location name → first matching node index (case-insensitive).</summary>
    private readonly Dictionary<string, int> _nameToIndex = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>Node index → outgoing edges.</summary>
    private readonly List<List<RouteGraphEdge>> _adjacency = new();

    /// <summary>Whether <see cref="Build"/> has been called.</summary>
    private bool _built;

    // ── Public accessors ───────────────────────────────────────────

    /// <summary>Number of graph nodes.</summary>
    public int NodeCount => _locations.Count;

    /// <summary>Location at the given node index.</summary>
    public Location GetLocation(int index) => _locations[index];

    /// <summary>Node index for a location name, or -1 if not in the graph.</summary>
    public int GetNodeIndex(string locationName)
    {
        return _nameToIndex.TryGetValue(locationName, out int idx) ? idx : -1;
    }

    /// <summary>Node index for a location, or -1 if not in the graph (matched by name).</summary>
    public int GetNodeIndex(Location location)
    {
        int idx = GetNodeIndex(location.Name);
        if (idx >= 0)
            return idx;
        // Fallback: scan by reference / equality
        for (int i = 0; i < _locations.Count; i++)
            if (ReferenceEquals(_locations[i], location) || _locations[i] == location)
                return i;
        return -1;
    }

    /// <summary>Outgoing edges from the given node index.</summary>
    public IReadOnlyList<RouteGraphEdge> GetEdges(int nodeIndex) => _adjacency[nodeIndex];

    /// <summary>Whether the graph has been built.</summary>
    public bool IsBuilt => _built;

    // ── Build ───────────────────────────────────────────────────────

    /// <summary>
    /// Build (or rebuild) the graph from the database.
    /// </summary>
    /// <param name="db">The location database.</param>
    /// <param name="maxWalkDistance">
    /// Maximum straight-line distance (map units) for creating a walk edge.
    /// Locations farther apart than this are not directly connected by a walk edge.
    /// </param>
    public void Build(LocationDatabase db, double maxWalkDistance = 10.0)
    {
        _locations.Clear();
        _nameToIndex.Clear();
        _adjacency.Clear();

        var eligible = db.AllLocations
            .Where(l => l.UseInRouteFinding && !l.IsRetired && l.HasCoordinates)
            .Distinct(LocationNameComparer.Instance)  // deduplicate by name
            .OrderBy(l => l.Name, StringComparer.OrdinalIgnoreCase)
            .ThenBy(l => l.Id)
            .ToList();

        // Build node index
        foreach (var loc in eligible)
        {
            string name = loc.Name;
            if (!_nameToIndex.ContainsKey(name))
            {
                int idx = _locations.Count;
                _nameToIndex[name] = idx;
                _locations.Add(loc);
                _adjacency.Add(new List<RouteGraphEdge>());
            }
        }

        // ── Walk edges: connect nearby locations ────────────────
        for (int i = 0; i < _locations.Count; i++)
        {
            for (int j = i + 1; j < _locations.Count; j++)
            {
                double dist = _locations[i].DistanceTo(_locations[j]);
                if (dist <= maxWalkDistance && dist > 0)
                {
                    // Undirected: add both directions
                    _adjacency[i].Add(new RouteGraphEdge(i, j, RouteEdgeKind.Walk, dist, "Walk"));
                    _adjacency[j].Add(new RouteGraphEdge(j, i, RouteEdgeKind.Walk, dist, "Walk"));
                }
            }
        }

        // ── Portal edges ────────────────────────────────────────
        // Older PortalDevice records only identify the destination and device.
        // Records with an explicit Entrance/From location can participate in
        // graph routing without guessing where the device is located.
        foreach (var pd in db.PortalDevices)
        {
            if (string.IsNullOrWhiteSpace(pd.EntranceLocation))
                continue;

            int fromIdx = GetNodeIndex(pd.EntranceLocation);
            string exitName = string.IsNullOrWhiteSpace(pd.ExitLocation)
                ? pd.Destination
                : pd.ExitLocation;
            int toIdx = GetNodeIndex(exitName);
            if (fromIdx < 0 || toIdx < 0 || fromIdx == toIdx)
                continue;

            _adjacency[fromIdx].Add(
                new RouteGraphEdge(fromIdx, toIdx, RouteEdgeKind.Portal, 0.1, pd.Via));
        }

        // ── Route-start edges (recall / lifestone / allegiance) ──
        foreach (var rs in db.RouteStarts)
        {
            int fromIdx = GetNodeIndex(rs.From);
            int toIdx = GetNodeIndex(rs.Destination);
            if (fromIdx < 0 || toIdx < 0)
                continue;

            RouteEdgeKind kind = InferEdgeKind(rs.Via);
            double cost = kind switch
            {
                RouteEdgeKind.Walk => _locations[fromIdx].DistanceTo(_locations[toIdx]),
                RouteEdgeKind.Portal => 0.1,
                RouteEdgeKind.Recall => 0.05,
                RouteEdgeKind.Lifestone => 0.05,
                _ => 0.1,
            };
            _adjacency[fromIdx].Add(new RouteGraphEdge(fromIdx, toIdx, kind, cost, rs.Via));
        }

        _built = true;
    }

    // ── A* Shortest Path ───────────────────────────────────────────

    /// <summary>
    /// Find the shortest path between two nodes using A* with Euclidean
    /// distance heuristic. Returns the sequence of edges, or null if
    /// the destination is unreachable.
    /// </summary>
    public List<RouteGraphEdge>? FindShortestPath(int fromIndex, int toIndex)
    {
        EnsureBuilt();
        if (fromIndex < 0 || fromIndex >= _locations.Count)
            return null;
        if (toIndex < 0 || toIndex >= _locations.Count)
            return null;
        if (fromIndex == toIndex)
            return new List<RouteGraphEdge>(); // already there

        int n = _locations.Count;
        var gScore = new double[n];
        var fScore = new double[n];
        var cameFrom = new int[n];
        var cameFromEdge = new RouteGraphEdge?[n];
        Array.Fill(gScore, double.MaxValue);
        Array.Fill(fScore, double.MaxValue);
        Array.Fill(cameFrom, -1);

        gScore[fromIndex] = 0;
        fScore[fromIndex] = Heuristic(fromIndex, toIndex);

        // Priority queue: (fScore, gScore, nodeIndex, tieBreaker)
        var open = new SortedSet<(double f, double g, int node, int tie)>();

        // Need a way to modify priorities: we'll use a HashSet to track closed nodes
        // and just add duplicates to the SortedSet (skip when popped if already closed).
        var closed = new HashSet<int>();
        int tieCounter = 0;
        open.Add((fScore[fromIndex], gScore[fromIndex], fromIndex, tieCounter++));

        while (open.Count > 0)
        {
            var (_, _, current, _) = open.Min;
            open.Remove(open.Min);

            if (!closed.Add(current))
                continue; // already processed

            if (current == toIndex)
                return ReconstructPath(cameFrom, cameFromEdge, current);

            foreach (var edge in _adjacency[current])
            {
                int neighbor = edge.ToIndex;
                if (closed.Contains(neighbor))
                    continue;

                double tentativeG = gScore[current] + edge.Cost;
                if (tentativeG >= gScore[neighbor])
                    continue;

                // Better path found
                cameFrom[neighbor] = current;
                cameFromEdge[neighbor] = edge;
                gScore[neighbor] = tentativeG;
                fScore[neighbor] = tentativeG + Heuristic(neighbor, toIndex);

                open.Add((fScore[neighbor], gScore[neighbor], neighbor, tieCounter++));
            }
        }

        return null; // unreachable
    }

    /// <summary>
    /// Convenience: find shortest path by location names.
    /// </summary>
    public List<RouteGraphEdge>? FindShortestPath(string fromName, string toName)
    {
        int fromIdx = GetNodeIndex(fromName);
        int toIdx = GetNodeIndex(toName);
        if (fromIdx < 0 || toIdx < 0)
            return null;
        return FindShortestPath(fromIdx, toIdx);
    }

    /// <summary>
    /// Convert a shortest-path result (list of edges) into a <see cref="Route"/>.
    /// </summary>
    public Route ToRoute(List<RouteGraphEdge> path, string destinationName)
    {
        var route = new Route(destinationName);
        if (path.Count == 0)
        {
            // Already at destination – single-step route
            var loc = _locations[GetNodeIndex(destinationName)];
            if (loc != null)
                route.AddTravelStep(loc, loc, "Arrived");
            return route;
        }

        // Walk the path and create RouteSteps
        for (int i = 0; i < path.Count; i++)
        {
            var edge = path[i];
            var from = _locations[edge.FromIndex];
            var to = _locations[edge.ToIndex];

            switch (edge.Kind)
            {
                case RouteEdgeKind.Walk:
                    route.AddTravelStep(from, to, edge.Via);
                    break;
                case RouteEdgeKind.Portal:
                    route.AddPortalStep(from, to, edge.Via);
                    break;
                case RouteEdgeKind.Recall:
                case RouteEdgeKind.Lifestone:
                    route.AddRecallStep(from, to, edge.Via);
                    break;
            }
        }

        return route;
    }

    // ── Private helpers ─────────────────────────────────────────────

    private double Heuristic(int fromIndex, int toIndex)
    {
        return _locations[fromIndex].DistanceTo(_locations[toIndex]);
    }

    private List<RouteGraphEdge> ReconstructPath(int[] cameFrom, RouteGraphEdge?[] cameFromEdge, int current)
    {
        var edges = new List<RouteGraphEdge>();
        while (cameFrom[current] >= 0)
        {
            var edge = cameFromEdge[current];
            if (edge != null)
                edges.Add(edge);
            current = cameFrom[current];
        }
        edges.Reverse();
        return edges;
    }

    private void EnsureBuilt()
    {
        if (!_built)
            throw new InvalidOperationException("RouteGraph has not been built. Call Build() first.");
    }

    private static RouteEdgeKind InferEdgeKind(string via)
    {
        var lower = via.Trim().ToLowerInvariant();
        if (lower.Contains("recall") || lower.Contains("portal recall"))
            return RouteEdgeKind.Recall;
        if (lower.Contains("lifestone") || lower.Contains("life stone"))
            return RouteEdgeKind.Lifestone;
        if (lower.Contains("portal") || lower.Contains("device"))
            return RouteEdgeKind.Portal;
        return RouteEdgeKind.Walk;
    }

    /// <summary>
    /// Equality comparer that treats two Locations as equal when they
    /// have the same name (case-insensitive) and coordinates.
    /// Used to deduplicate nodes during Build.
    /// </summary>
    private sealed class LocationNameComparer : IEqualityComparer<Location>
    {
        public static readonly LocationNameComparer Instance = new();

        public bool Equals(Location? x, Location? y)
        {
            if (x is null || y is null) return x == y;
            return string.Equals(x.Name, y.Name, StringComparison.OrdinalIgnoreCase);
        }

        public int GetHashCode(Location obj) =>
            StringComparer.OrdinalIgnoreCase.GetHashCode(obj.Name);
    }
}