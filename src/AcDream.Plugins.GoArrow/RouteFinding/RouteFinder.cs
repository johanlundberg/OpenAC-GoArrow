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
    private readonly double _portalSearchRadius;

    /// <summary>
    /// Maximum portal search radius in map units. Default 2.5 mu (~600m).
    /// </summary>
    public const double DefaultPortalSearchRadius = 2.5;

    public RouteFinder(LocationDatabase database, double portalSearchRadius = DefaultPortalSearchRadius)
    {
        _database = database;
        _portalSearchRadius = portalSearchRadius;
    }

    /// <summary>
    /// Find a route from the current position to a named destination.
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
    /// </summary>
    public Route FindRoute(Location currentPosition, Location destination)
    {
        var route = new Route(destination.Name);

        // Check for a direct route start from current position's nearest
        // named location.
        var nearestFrom = FindNearestNamedLocation(currentPosition);
        if (nearestFrom != null)
        {
            var directRoute = TryFindDirectRoute(nearestFrom, destination);
            if (directRoute != null)
                return directRoute;
        }

        // Try portal-based routing: find a portal near current position
        // that leads to a location near the destination.
        var portalRoute = TryFindPortalRoute(currentPosition, destination);
        if (portalRoute != null)
            return portalRoute;

        // Fallback: direct travel (walk there).
        route.AddTravelStep(currentPosition, destination, "Walk");
        return route;
    }

    /// <summary>
    /// Find the nearest named location in the database.
    /// </summary>
    public Location? FindNearestNamedLocation(Location position)
    {
        Location? nearest = null;
        double nearestDist = double.MaxValue;

        foreach (var loc in _database.AllLocations)
        {
            if (!loc.UseInRouteFinding || loc.IsRetired)
                continue;

            double dist = position.DistanceTo(loc);
            if (dist < nearestDist && dist < _portalSearchRadius * 2)
            {
                nearestDist = dist;
                nearest = loc;
            }
        }

        return nearest;
    }

    private Route? TryFindDirectRoute(Location from, Location to)
    {
        // Check route starts table
        var routeStarts = _database.FindRoutesFrom(from.Name);
        foreach (var rs in routeStarts)
        {
            if (string.Equals(rs.Destination, to.Name, StringComparison.OrdinalIgnoreCase))
            {
                // Found a direct route start via a recall or portal
                var viaLoc = _database.FindLocation(rs.Via);
                if (viaLoc != null)
                {
                    var route = new Route(to.Name);
                    route.AddRecallStep(from, viaLoc, rs.Via);
                    route.AddTravelStep(viaLoc, to, "Arrive");
                    return route;
                }
                else
                {
                    // Maybe Via is a portal device name, not a location
                    var portalDest = ResolvePortalDestination(rs.Via);
                    if (portalDest != null)
                    {
                        var route = new Route(to.Name);
                        route.AddPortalStep(from, portalDest, rs.Via);
                        double dist = portalDest.DistanceTo(to);
                        if (dist > 0.1)
                            route.AddTravelStep(portalDest, to, "Arrive");
                        return route;
                    }
                }
            }
        }

        // Check if destination is reachable directly from a nearby location
        double directDist = from.DistanceTo(to);
        if (directDist < _portalSearchRadius * 3)
        {
            var route = new Route(to.Name);
            route.AddTravelStep(from, to, "Walk");
            return route;
        }

        return null;
    }

    private Route? TryFindPortalRoute(Location currentPosition, Location destination)
    {
        // Find portal devices near the destination
        var portalsTo = _database.FindPortalsTo(destination.Name);
        if (portalsTo.Count == 0)
        {
            // Try fuzzy match: find portals whose destination name
            // is contained in, or contains, our destination name
            var allPortals = _database.PortalDevices;
            foreach (var pd in allPortals)
            {
                if (pd.Destination.IndexOf(destination.Name, StringComparison.OrdinalIgnoreCase) >= 0
                    || destination.Name.IndexOf(pd.Destination, StringComparison.OrdinalIgnoreCase) >= 0)
                {
                    portalsTo.Add(pd);
                }
            }
        }

        // Find a portal device near our current position
        var portalsFromCurrent = FindPortalDevicesNear(currentPosition);
        if (portalsTo.Count > 0)
        {
            // Build route using the portal device
            var pd = portalsTo[0]; // Use first match
            var portalLocation = _database.FindLocation(pd.Destination) ?? destination;

            var route = new Route(destination.Name);

            // Step to nearest walkable location en route to portal
            if (portalsFromCurrent.Count > 0)
            {
                var nearestPortal = portalsFromCurrent[0];
                var portalLoc = _database.FindLocation(nearestPortal.Destination);
                if (portalLoc != null)
                {
                    route.AddTravelStep(currentPosition, portalLoc, "Walk to portal");
                    route.AddPortalStep(portalLoc, portalLocation, nearestPortal.Via);
                }
                else
                {
                    route.AddTravelStep(currentPosition, destination, "Walk (no portal location)");
                }
            }
            else
            {
                route.AddTravelStep(currentPosition, destination, "Walk to destination");
            }

            // Final leg
            double finalDist = portalLocation.DistanceTo(destination);
            if (finalDist > 0.1)
                route.AddTravelStep(portalLocation, destination, "Arrive");

            return route;
        }

        return null;
    }

    /// <summary>
    /// Find portal devices near a given position.
    /// </summary>
    public List<PortalDevice> FindPortalDevicesNear(Location position)
    {
        var result = new List<PortalDevice>();
        var portalDestinations = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        foreach (var pd in _database.PortalDevices)
        {
            var dest = _database.FindLocation(pd.Destination);
            if (dest != null && position.DistanceTo(dest) < _portalSearchRadius)
            {
                if (portalDestinations.Add(pd.Destination))
                    result.Add(pd);
            }
        }

        return result;
    }

    private Location? ResolvePortalDestination(string portalName)
    {
        // Check if the portal name matches a location name
        var loc = _database.FindLocation(portalName);
        if (loc != null)
            return loc;

        // Check portal devices for a via matching the name
        foreach (var pd in _database.PortalDevices)
        {
            if (string.Equals(pd.Via, portalName, StringComparison.OrdinalIgnoreCase))
            {
                return _database.FindLocation(pd.Destination);
            }
        }

        return null;
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