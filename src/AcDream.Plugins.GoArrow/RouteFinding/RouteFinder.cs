using System.Globalization;

namespace AcDream.Plugins.GoArrow.RouteFinding;

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
    /// Uses A* shortest path on the route graph when possible.
    /// </summary>
    public Route FindRoute(Location currentPosition, string destinationName)
    {
        var dest = _database.FindLocation(destinationName);
        if (dest == null)
            return new Route(destinationName) { Description = $"Destination '{destinationName}' not found." };

        return FindRoute(currentPosition, dest);
    }

    /// <summary>
    /// Find a route from the current position to a destination location.
    /// Uses A* shortest path on the route graph when possible,
    /// falling back to direct walk if the graph doesn't cover the route.
    /// </summary>
    public Route FindRoute(Location currentPosition, Location destination)
    {
        EnsureGraphBuilt();

        if (!destination.HasCoordinates)
            return new Route(destination.Name) { Description = $"Destination '{destination.Name}' has no coordinates." };

        // Try graph-based route finding
        var nearestFrom = FindNearestNamedLocation(currentPosition);
        if (nearestFrom != null)
        {
            int fromIdx = _graph.GetNodeIndex(nearestFrom);
            int toIdx = _graph.GetNodeIndex(destination);

            if (fromIdx >= 0 && toIdx >= 0)
            {
                var path = _graph.FindShortestPath(fromIdx, toIdx);
                if (path != null)
                {
                    var route = _graph.ToRoute(path, destination.Name);

                    // Prepend walk from current position to nearest named location
                    double distToNearest = currentPosition.DistanceTo(nearestFrom);
                    if (distToNearest > 0.01)
                    {
                        route.PrependStep(new RouteStep(
                            RouteStepKind.Travel, currentPosition, nearestFrom,
                            distToNearest, "Walk to start"));
                    }

                    return route;
                }
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
        return _database.AllLocations
            .Where(l => l.UseInRouteFinding && !l.IsRetired)
            .Select(l => l.Name)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .OrderBy(n => n)
            .ToList();
    }
}