using System.Globalization;

namespace AcDream.Plugins.GoArrow.RouteFinding;

/// <summary>Deterministic weighting profile for graph route selection.</summary>
public enum RouteCostProfile
{
    ShortestWalk,
    FewestInteractions,
    PreferRecall,
    AvoidInteractions,
}

/// <summary>
/// Finds the best route between two locations using the location database,
/// portal device table, and route start table.
/// Ported from GoArrow RouteFinder.cs — Decal dependencies removed.
/// </summary>
public class RouteFinder
{
    private readonly LocationDatabase _database;
    private readonly double _maxWalkDistance;
    private RouteGraph _graph;
    private bool _graphBuilt;

    /// <summary>
    /// Default maximum walk distance between graph nodes (map units). ~2.4 km.
    /// </summary>
    public const double DefaultWalkDistance = 10.0;

    public RouteFinder(LocationDatabase database, double maxWalkDistance = DefaultWalkDistance)
    {
        _database = database;
        _maxWalkDistance = maxWalkDistance;
        _graph = new RouteGraph();
    }

    /// <summary>
    /// Find a route from the current position to a named destination.
    /// Uses shortest path on the route graph when possible.
    /// </summary>
    public Route FindRoute(Location currentPosition, string destinationName)
    {
        var dest = _database.FindLocation(destinationName);
        if (dest == null)
            return new Route(destinationName)
            {
                Description = $"Destination '{destinationName}' not found.",
            };

        return FindRoute(currentPosition, dest);
    }

    /// <summary>
    /// Find a route from the current position to a destination location.
    /// Uses shortest path on the route graph when possible,
    /// falling back to direct walk if the graph doesn't cover the route.
    /// </summary>
    public Route FindRoute(
        Location currentPosition,
        Location destination,
        RouteCostProfile profile = RouteCostProfile.ShortestWalk
    )
    {
        EnsureGraphBuilt();

        if (!destination.HasCoordinates)
            return new Route(destination.Name)
            {
                Description = $"Destination '{destination.Name}' has no coordinates.",
            };

        // Connect this exact origin to all nearby graph nodes for this search.
        // Choosing one named location before Dijkstra can miss a better portal
        // or add an artificial detour at the start of the route.
        int toIdx = _graph.GetNodeIndex(destination);
        if (toIdx >= 0)
        {
            Func<RouteGraphEdge, double>? weighting =
                profile == RouteCostProfile.ShortestWalk
                    ? null
                    : edge =>
                        profile switch
                        {
                            RouteCostProfile.FewestInteractions => edge.Kind == RouteEdgeKind.Walk
                                ? 1d
                                : 0.1d,
                            RouteCostProfile.PreferRecall => edge.Kind
                                is RouteEdgeKind.Recall
                                    or RouteEdgeKind.Lifestone
                                ? edge.Cost * 0.1d
                                : edge.Cost,
                            RouteCostProfile.AvoidInteractions => edge.Kind == RouteEdgeKind.Walk
                                ? edge.Cost
                                : edge.Cost * 1000d,
                            _ => edge.Cost,
                        };
            var path = _graph.FindShortestPathFromPosition(
                currentPosition,
                toIdx,
                _maxWalkDistance,
                weighting
            );
            if (path is { Count: > 0 })
            {
                var connection = path[0];
                Location entry = _graph.GetLocation(connection.ToIndex);
                if (path.Count == 1)
                {
                    var directRoute = new Route(destination.Name);
                    directRoute.AddTravelStep(
                        currentPosition,
                        entry,
                        connection.Cost == 0 ? "Arrived" : "Walk"
                    );
                    return directRoute;
                }

                var route = _graph.ToRoute(path.Skip(1).ToList(), destination.Name);
                if (connection.Cost > 0)
                {
                    route.PrependStep(
                        new RouteStep(
                            RouteStepKind.Travel,
                            currentPosition,
                            entry,
                            connection.Cost,
                            "Walk to start"
                        )
                    );
                }
                else
                    route.SetFirstStepOrigin(currentPosition);
                return route;
            }
        }

        // Fallback: direct travel (walk there).
        var fallbackRoute = new Route(destination.Name);
        fallbackRoute.AddTravelStep(currentPosition, destination, "Walk");
        fallbackRoute.Description = "Direct walk (no graph route found)";
        return fallbackRoute;
    }

    /// <summary>
    /// Find the nearest eligible named location for graph routing.
    /// </summary>
    public Location? FindNearestNamedLocation(Location position)
    {
        Location? nearest = null;
        double nearestDist = double.MaxValue;

        foreach (var loc in _database.AllLocations)
        {
            if (!loc.UseInRouteFinding || loc.IsRetired || !loc.HasCoordinates)
                continue;

            double dist = position.DistanceTo(loc);
            if (dist < nearestDist && dist < _maxWalkDistance * 2)
            {
                nearestDist = dist;
                nearest = loc;
            }
        }

        return nearest;
    }

    /// <summary>Atomically discards the graph so the next route sees a complete snapshot.</summary>
    public void InvalidateGraph() => _graphBuilt = false;

    private void EnsureGraphBuilt()
    {
        if (!_graphBuilt)
        {
            var rebuilt = new RouteGraph();
            rebuilt.Build(_database, _maxWalkDistance);
            _graph = rebuilt;
            _graphBuilt = true;
        }
    }

    /// <summary>
    /// Search for locations by name substring.
    /// </summary>
    public List<Location> SearchLocations(string query)
    {
        return _database.SearchLocations(query);
    }

    /// <summary>
    /// Get a summary of all known destinations.
    /// </summary>
    public List<string> GetAllDestinationNames()
    {
        return _database
            .AllLocations.Where(l => l.UseInRouteFinding && !l.IsRetired)
            .Select(l => l.Name)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .OrderBy(n => n)
            .ToList();
    }
}
