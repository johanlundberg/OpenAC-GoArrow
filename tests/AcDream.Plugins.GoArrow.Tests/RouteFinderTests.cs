using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public class RouteFinderTests
{
    private static (LocationDatabase, RouteFinder) CreateTestDb()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(new[]
        {
            "TownA;0;0",
            "TownB;10;0",
            "TownC;10;10",
            "TownD;0;10",
            "PortalDest;5;5",
        });
        db.LoadPortalDevicesCsv(new[]
        {
            "PortalDest;Magic Portal;Dereth",
        });
        db.LoadRouteStartsCsv(new[]
        {
            "TownB;TownA;Walk",
        });

        var finder = new RouteFinder(db);
        return (db, finder);
    }

    [Fact]
    public void RouteFinder_FindRouteToKnownDestination_ReturnsRoute()
    {
        var (db, finder) = CreateTestDb();
        var currentPos = new Location("Current", 0, 0);

        var route = finder.FindRoute(currentPos, "TownB");

        Assert.NotNull(route);
        Assert.Equal("TownB", route.Destination);
    }

    [Fact]
    public void RouteFinder_FindRouteToUnknownDestination_ReturnsEmptyRoute()
    {
        var (db, finder) = CreateTestDb();
        var currentPos = new Location("Current", 0, 0);

        var route = finder.FindRoute(currentPos, "Nowhere");

        Assert.NotNull(route);
        Assert.Contains("not found", route.Description, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void RouteFinder_FindNearestNamedLocation_ReturnsClosest()
    {
        var (db, finder) = CreateTestDb();
        var nearTownA = new Location("Near A", 0.5, 0.5);

        var nearest = finder.FindNearestNamedLocation(nearTownA);

        Assert.NotNull(nearest);
        Assert.Equal("TownA", nearest.Name);
    }

    [Fact]
    public void RouteFinder_SearchLocations_ReturnsMatching()
    {
        var (db, finder) = CreateTestDb();
        var results = finder.SearchLocations("Town");
        Assert.Equal(4, results.Count);
    }

    [Fact]
    public void RouteFinder_GetAllDestinationNames_ReturnsDistinct()
    {
        var (db, finder) = CreateTestDb();
        var names = finder.GetAllDestinationNames();

        // The CSV has 5 locations: TownA, TownB, TownC, TownD, PortalDest
        Assert.Equal(5, names.Count);
        Assert.Contains("TownA", names);
        Assert.Contains("TownC", names);
    }

    [Fact]
    public void RouteFinder_GraphBasedRoute_UsesWalkEdges()
    {
        var (db, finder) = CreateTestDb();
        // TownA (0,0) and TownB (10,0) are within DefaultWalkDistance (10.0)
        var atTownA = new Location("At Town A", 0, 0);

        var route = finder.FindRoute(atTownA, "TownC");

        Assert.NotNull(route);
        Assert.Equal("TownC", route.Destination);
        Assert.True(route.StepCount > 0);
        // Should find a multi-hop route through nearby towns
        Assert.True(route.TotalDistance > 0);
        Assert.Equal(0, route.PortalCount);
    }

    [Fact]
    public void RouteFinder_FindRouteFindsDirectRouteStart()
    {
        var (db, finder) = CreateTestDb();
        var atTownA = new Location("At Town A", 0, 0);

        // The route starts table says TownB can be reached from TownA via Walk.
        // Graph should find this as a walk edge (they're within 10mu).
        var route = finder.FindRoute(atTownA, "TownB");

        Assert.NotNull(route);
        Assert.Equal("TownB", route.Destination);
        Assert.True(route.StepCount > 0);
        // Current position (0,0) is AT TownA (0,0), so no walk-to-start step.
        // Direct graph edge: TownA → TownB via Walk
        Assert.Contains("Walk", route.Steps[0].Via);
    }
}