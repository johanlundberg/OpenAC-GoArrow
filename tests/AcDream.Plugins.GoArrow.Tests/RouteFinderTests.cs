using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public class RouteFinderTests
{
    private static (LocationDatabase, RouteFinder) CreateTestDb()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(
            new[] { "TownA;0;0", "TownB;10;0", "TownC;10;10", "TownD;0;10", "PortalDest;5;5" }
        );
        db.LoadPortalDevicesCsv(new[] { "PortalDest;Magic Portal;Dereth" });
        db.LoadRouteStartsCsv(new[] { "TownB;TownA;Walk" });

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
    public void RouteStartsAtExactCurrentCoordinatesEvenForTinyOffset()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(new[] { "Start;0;0", "Middle;0;8", "End;0;16" });
        var current = new Location("Current Position", 0.003, 0.004);

        Route route = new RouteFinder(db).FindRoute(current, "End");

        Assert.NotEmpty(route.Steps);
        Assert.Same(current, route.Steps[0].From);
        Assert.Equal(0.003, route.Steps[0].From.NS);
        Assert.Equal(0.004, route.Steps[0].From.EW);
    }

    [Fact]
    public void RouteKeepsExactOriginWhenStandingOnAGraphNode()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(new[] { "Start;0;0", "Middle;0;8", "End;0;16" });
        var current = new Location("Current Position", 0, 0);

        Route route = new RouteFinder(db).FindRoute(current, "End");

        Assert.Same(current, route.Steps[0].From);
        Assert.Equal(0, route.Steps[0].From.NS);
        Assert.Equal(0, route.Steps[0].From.EW);
    }

    [Fact]
    public void RouteCanEnterGraphAwayFromNearestNamedLocation()
    {
        var db = new LocationDatabase();
        db.LoadLocationsXml(
            """
            <atlas>
              <location><id>1</id><name>Nearest Town</name><type>Town</type><latitude>0</latitude><longitude>-1</longitude><retired>N</retired></location>
              <location><id>2</id><name>Useful Portal</name><type>Wilderness Portal</type><latitude>0</latitude><longitude>5</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>95</arrival_longitude><retired>N</retired></location>
              <location><id>3</id><name>End</name><type>Town</type><latitude>0</latitude><longitude>96</longitude><retired>N</retired></location>
            </atlas>
            """
        );
        var current = new Location("Current Position", 0, 0);

        Route route = new RouteFinder(db).FindRoute(current, "End");

        Assert.Equal("Useful Portal", route.Steps[0].To.Name);
        Assert.Same(current, route.Steps[0].From);
        Assert.Equal(1, route.PortalCount);
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

    [Fact]
    public void RouteFinder_UsesAtlasPortalEntranceAndArrivalCoordinates()
    {
        var db = new LocationDatabase();
        db.LoadLocationsXml(
            """
            <atlas>
              <location><id>1</id><name>Start</name><type>Town</type><latitude>0</latitude><longitude>1</longitude><retired>N</retired></location>
              <location><id>2</id><name>Shared Portal</name><type>Wilderness Portal</type><latitude>0</latitude><longitude>20</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>50</arrival_longitude><retired>N</retired></location>
              <location><id>3</id><name>Shared Portal</name><type>Wilderness Portal</type><latitude>0</latitude><longitude>2</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>95</arrival_longitude><retired>N</retired></location>
              <location><id>4</id><name>End</name><type>Town</type><latitude>0</latitude><longitude>96</longitude><retired>N</retired></location>
            </atlas>
            """
        );

        var route = new RouteFinder(db).FindRoute(new Location("Here", 0, 1), "End");

        Assert.Equal(1, route.PortalCount);
        Assert.Equal(3, route.StepCount);
        Assert.Equal(RouteStepKind.Travel, route.Steps[0].Kind);
        Assert.Equal(2, route.Steps[0].To.EW);
        Assert.Equal(RouteStepKind.Portal, route.Steps[1].Kind);
        Assert.Equal("Shared Portal", route.Steps[1].Via);
        Assert.Equal(2, route.Steps[1].From.EW);
        Assert.Equal(95, route.Steps[1].To.EW);
        Assert.Equal(96, route.Steps[2].To.EW);
    }

    [Fact]
    public void RouteFinder_IgnoresRetiredAndUnmappedAtlasPortals()
    {
        var db = new LocationDatabase();
        db.LoadLocationsXml(
            """
            <atlas>
              <location><id>1</id><name>Start</name><type>Town</type><latitude>0</latitude><longitude>1</longitude><retired>N</retired></location>
              <location><id>2</id><name>Retired Portal</name><type>Town Portal</type><latitude>0</latitude><longitude>2</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>95</arrival_longitude><retired>Y</retired></location>
              <location><id>3</id><name>Unknown Exit</name><type>Town Portal</type><latitude>0</latitude><longitude>3</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>0</arrival_longitude><retired>N</retired></location>
              <location><id>4</id><name>End</name><type>Town</type><latitude>0</latitude><longitude>96</longitude><retired>N</retired></location>
            </atlas>
            """
        );

        var route = new RouteFinder(db).FindRoute(new Location("Here", 0, 1), "End");

        Assert.Equal(0, route.PortalCount);
        Assert.Contains("Direct walk", route.Description);
    }
}
