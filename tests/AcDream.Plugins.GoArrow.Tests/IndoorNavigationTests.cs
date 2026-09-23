using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class IndoorNavigationTests
{
    [Fact]
    public void NamedSawatoLocationWalksToSawatoPortalInsideTownNetwork()
    {
        var host = HostAt(0x12340100);
        host.HasUiValue = false;
        var portals = new PortalWorldObjects();
        portals.Objects.Add(Portal(41, "Holtburg Portal", 0x12340111, 10.001));
        portals.Objects.Add(Portal(42, "Portal to Sawato", 0x12340122, 10.01));
        portals.Objects.Add(Portal(43, "Sawato Portal", 0x56780122, 10.0001));
        host.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.PluginNavigation,
            Chat = host.PluginChat,
            Objects = portals,
        };
        new GoArrowSettings { AutoNavigate = true }.Save(host.PluginStorage);
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();
        Assert.True(plugin.SetDestination("Sawato"));

        plugin.Panel!.StartNavigation();

        Assert.Equal((uint)42, Assert.Single(host.PluginNavigation.GoToCalls).ObjectId);
        Assert.Empty(host.PluginNavigation.GoToPositionCalls);
        Assert.Equal("Client planning path...", plugin.Panel.NavStatusText);
        plugin.Disable();
    }

    [Fact]
    public void NamedLocationWithoutMatchingLocalPortalDoesNotUseOutdoorCoordinateIndoors()
    {
        var host = HostAt(0x12340100);
        var portals = new PortalWorldObjects();
        portals.Objects.Add(Portal(41, "Holtburg Portal", 0x12340111, 10.001));
        host.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.PluginNavigation,
            Chat = host.PluginChat,
            Objects = portals,
        };
        var settings = new GoArrowSettings();
        var database = new LocationDatabase();
        database.LoadLocationsXml("<locations><loc name='Sawato Portal' type='TownPortal' NS='-28.7' EW='59.3' /></locations>");
        var destination = new GoArrowDestination(settings, database, new RouteFinder(database));
        Assert.True(destination.SetDestination("Sawato Portal"));
        using var navigator = new GoArrowNavigator(host, destination, settings);

        navigator.StartNavigation();

        Assert.Empty(host.PluginNavigation.GoToCalls);
        Assert.Empty(host.PluginNavigation.GoToPositionCalls);
    }

    [Fact]
    public void MarkedIndoorPointRemainsNamedAfterLocationDataReloadAndPluginRestart()
    {
        var host = HostAt(0x12340122);
        host.HasUiValue = false;
        new GoArrowSettings { AutoNavigate = true }.Save(host.PluginStorage);
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();
        Assert.True(plugin.MarkCurrentIndoorLocation("Lower Chamber"));
        Assert.Contains("cellId=\"0x12340122\"", host.PluginStorage.ReadText("GoArrow/indoor-locations.xml"));

        host.PluginStorage.WriteText("GoArrow/base.xml", """
            <locations><loc name="Other Place" NS="1" EW="2" /></locations>
            """);
        Assert.True(plugin.LoadDataFile("base.xml"));
        Assert.True(plugin.SetDestination("Lower Chamber"));
        host.PluginNavigation.SnapshotValue = host.PluginNavigation.SnapshotValue with
        {
            Position = new PluginNavigationPosition(0x12340100, 10, 10, 0, 0, false)
        };
        plugin.Panel!.StartNavigation();
        Assert.Equal((uint)0x12340122, Assert.Single(host.PluginNavigation.GoToPositionCalls).Position.CellId);
        plugin.Disable();

        var restarted = new GoArrowPlugin();
        restarted.Initialize(host);
        Assert.True(restarted.SetDestination("Lower Chamber"));
        Assert.Contains(restarted.SearchLocations("Lower"), location => location.Name == "Lower Chamber");
    }

    [Fact]
    public void LoadedLocationCanBeSelectedByNameAndWalkedFromPanel()
    {
        var host = HostAt(0x12340100);
        host.HasUiValue = false;
        new GoArrowSettings { AutoNavigate = true }.Save(host.PluginStorage);
        host.PluginStorage.WriteText("GoArrow/indoor.xml", """
            <locations><loc name="Dungeon Chest" type="Dungeon"
              cellId="0x12340122" x="35" y="50" z="6" /></locations>
            """);
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();

        Assert.True(plugin.LoadDataFile("indoor.xml"));
        Assert.Contains(plugin.SearchLocations("Chest"), location => location.Name == "Dungeon Chest");
        Assert.True(plugin.SetDestination("Dungeon Chest"));
        plugin.Panel!.StartNavigation();

        Assert.Equal((uint)0x12340122, Assert.Single(host.PluginNavigation.GoToPositionCalls).Position.CellId);
        Assert.Equal("Client planning path...", plugin.Panel.NavStatusText);
        plugin.Disable();
    }

    [Fact]
    public void NamedLocationWithIndoorCellWalksAcrossCellsOnItsFloor()
    {
        var host = HostAt(0x12340100);
        var database = new LocationDatabase();
        database.LoadLocationsXml("""
            <locations>
              <loc name="Dungeon Chest" type="Dungeon" cellId="0x12340122" x="35" y="50" z="6" />
            </locations>
            """);
        var settings = new GoArrowSettings();
        var destination = new GoArrowDestination(settings, database, new RouteFinder(database));
        Assert.True(destination.SetDestination("Dungeon Chest"));
        using var navigator = new GoArrowNavigator(host, destination, settings);

        navigator.StartNavigation();

        var walk = Assert.Single(host.PluginNavigation.GoToPositionCalls);
        Assert.Equal((uint)0x12340122, walk.Position.CellId);
        var local = PluginDungeonFloorplan.ToLandblockLocal(walk.Position);
        Assert.Equal(35, local.X, 3);
        Assert.Equal(50, local.Y, 3);
        Assert.Equal(6, local.Z, 3);
        Assert.True(navigator.IsNavigating);
        Assert.Empty(host.PluginNavigation.GoToCalls);
    }

    [Fact]
    public void NamedIndoorLocationInAnotherDungeonIsNotMappedToOutdoorCoordinates()
    {
        var host = HostAt(0x12340100);
        var database = new LocationDatabase();
        database.LoadLocationsXml("""
            <locations><loc name="Other Dungeon Chest" NS="10" EW="10"
              cellId="0x56780122" x="35" y="50" z="6" /></locations>
            """);
        var settings = new GoArrowSettings();
        var destination = new GoArrowDestination(settings, database, new RouteFinder(database));
        Assert.True(destination.SetDestination("Other Dungeon Chest"));
        using var navigator = new GoArrowNavigator(host, destination, settings);

        navigator.StartNavigation();

        Assert.Empty(host.PluginNavigation.GoToPositionCalls);
        Assert.False(navigator.IsNavigating);
    }

    [Fact]
    public void SelectedObjectInSameDungeonUsesHostObjectPathAndArrives()
    {
        var host = HostAt(0x12340100);
        var (destination, navigator) = CreateNavigator(host, ObjectAt(0x12340122));
        using (navigator)
        {
            navigator.Enable();
            navigator.StartNavigation();

            Assert.True(navigator.IsNavigating);
            Assert.Equal((uint)42, Assert.Single(host.PluginNavigation.GoToCalls).ObjectId);
            Assert.Empty(host.PluginNavigation.GoToPositionCalls);
            Assert.Null(destination.CurrentRoute);

            host.PluginEvents.RaiseNavigationChanged(new PluginGoToReport(
                1, PluginGoToState.Arrived, 42, 0, 0, null) { Revision = 1 });

            Assert.True(navigator.HasArrived);
            Assert.False(navigator.IsNavigating);
            Assert.Single(host.PluginNavigation.GoToCalls);
        }
    }

    [Theory]
    [InlineData(0x56780122u, false)]
    [InlineData(0x12340122u, true)]
    public void IndoorObjectOutsideCurrentDungeonOrOutdoorsIsNotRouted(uint targetCell, bool targetOutdoor)
    {
        var host = HostAt(0x12340100);
        var (_, navigator) = CreateNavigator(host, ObjectAt(targetCell, targetOutdoor));
        using (navigator)
        {
            navigator.StartNavigation();

            Assert.False(navigator.IsNavigating);
            Assert.Empty(host.PluginNavigation.GoToCalls);
            Assert.Empty(host.PluginNavigation.GoToPositionCalls);
        }
    }

    [Fact]
    public void StoppingIndoorWalkCancelsHostRequest()
    {
        var host = HostAt(0x12340100);
        var (_, navigator) = CreateNavigator(host, ObjectAt(0x12340122));
        using (navigator)
        {
            navigator.StartNavigation();
            navigator.StopNavigation();

            Assert.False(navigator.IsNavigating);
            Assert.Equal(1, host.PluginNavigation.StopGoToCount);

            navigator.ResumeNavigation();

            Assert.True(navigator.IsNavigating);
            Assert.Equal(2, host.PluginNavigation.GoToCalls.Count);
        }
    }

    private static FakePluginHost HostAt(uint cellId)
    {
        var host = new FakePluginHost();
        host.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(cellId, 10, 10, 0, 0, false), false, false);
        return host;
    }

    private static PluginWorldObject ObjectAt(uint cellId, bool isOutdoor = false) =>
        new(42, 0, "Dungeon Target", PluginObjectClass.Npc, 0, 0, 0)
        {
            HasPosition = true,
            Position = new PluginNavigationPosition(cellId, 10.01, 10.01, 0, 0, isOutdoor)
        };

    private static PluginWorldObject Portal(uint id, string name, uint cellId, double eastWest) =>
        new(id, 0, name, PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(cellId, eastWest, 10, 0, 0, false),
        };

    private sealed class PortalWorldObjects : IWorldObjectAutomation
    {
        public List<PluginWorldObject> Objects { get; } = [];
        public IReadOnlyList<PluginWorldObject> CaptureObjects() => Objects;
    }

    private static (GoArrowDestination Destination, GoArrowNavigator Navigator) CreateNavigator(
        FakePluginHost host, PluginWorldObject target)
    {
        var settings = new GoArrowSettings();
        var database = new LocationDatabase();
        var destination = new GoArrowDestination(settings, database, new RouteFinder(database));
        Assert.True(destination.SetObject(target));
        return (destination, new GoArrowNavigator(host, destination, settings));
    }
}
