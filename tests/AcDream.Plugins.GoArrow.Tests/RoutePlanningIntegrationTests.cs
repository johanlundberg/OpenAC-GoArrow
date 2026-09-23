using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class RoutePlanningIntegrationTests
{
    [Fact]
    public void AutoNavigateWaitsForGoAfterDestinationChanges()
    {
        var host = new FakePluginHost { HasUiValue = false };
        new GoArrowSettings
        {
            AutoNavigate = true,
            DestinationName = "Holtburg",
        }.Save(host.PluginStorage);
        host.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(0, 0, 0, 0, 0, true), false, false);
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();

        Assert.Equal("Holtburg", plugin.CurrentDestinationName);
        Assert.Empty(host.PluginNavigation.GoToPositionCalls);
        Assert.True(plugin.SetDestination("Arwic"));
        Assert.Empty(host.PluginNavigation.GoToPositionCalls);

        plugin.Panel!.StartNavigation();
        Assert.NotEmpty(host.PluginNavigation.GoToPositionCalls);
        int startedLegs = host.PluginNavigation.GoToPositionCalls.Count;
        Assert.True(plugin.SetDestination("Holtburg"));
        Assert.Equal(1, host.PluginNavigation.StopGoToCount);
        Assert.Equal(startedLegs, host.PluginNavigation.GoToPositionCalls.Count);

        Assert.True(plugin.TrySetCoordinateDestination("42.1N 33.6E"));
        Assert.Equal(startedLegs, host.PluginNavigation.GoToPositionCalls.Count);
        plugin.Disable();
    }

    [Fact]
    public void ConfigTabSavesDownloadUrlsAndRejectsInvalidInput()
    {
        var host = new FakePluginHost { HasUiValue = false };
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        GoArrowPanel panel = plugin.Panel!;

        Assert.True(panel.RouteTabSelected);
        Assert.Equal(string.Empty, panel.LocationDataUrlInput);
        Assert.Equal(string.Empty, panel.DungeonMapUrlInput);
        panel.ShowConfigTab();
        Assert.True(panel.ConfigTabSelected);
        Assert.False(panel.RouteTabVisible);
        panel.SubmitLocationDataUrlAction("https://example.test/locations.xml");
        panel.SubmitDungeonMapUrlAction("https://example.test/maps.zip");
        GoArrowSettings saved = host.PluginStorage.ReadJson<GoArrowSettings>("settings.json")!;
        Assert.Equal("https://example.test/locations.xml", saved.ExternalDataUrl);
        Assert.Equal("https://example.test/maps.zip", saved.DungeonMapUrl);

        panel.SubmitDungeonMapUrlAction("file:///tmp/maps.zip");
        Assert.Contains("valid", panel.DungeonDownloadStatus);
        Assert.Equal("https://example.test/maps.zip",
            host.PluginStorage.ReadJson<GoArrowSettings>("settings.json")!.DungeonMapUrl);
        panel.ShowRouteTab();
        Assert.True(panel.RouteTabSelected);
    }

    [Fact]
    public void SearchFieldsPlanFromNamedPlaceToCurrentLocationWithoutMoving()
    {
        var host = new FakePluginHost { HasUiValue = false };
        host.PluginStorage.WriteText("GoArrow/test.xml", """
            <Locations><Location name="Test Start"><Coords NS="1" EW="1" /></Location>
            <Location name="Test End"><Coords NS="10" EW="10" /></Location></Locations>
            """);
        host.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(0, 8, 8, 0, 0, true), false, false);
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();
        Assert.True(plugin.LoadDataFile("test.xml"));
        GoArrowPanel panel = plugin.Panel!;

        Assert.Equal("Current Location", panel.FromInput);
        Assert.Equal("Current Location", panel.FromText);
        Assert.True(panel.FromSelectionVisible);
        Assert.False(panel.FromEditorVisible);
        panel.EditFromAction();
        Assert.True(panel.FromEditorVisible);
        Assert.Equal(string.Empty, panel.FromEditorInput);
        panel.UpdateFromInputAction("Test Sta");
        Assert.Contains("Test Start", panel.SearchResults);
        panel.SelectSearchResultAction(panel.SearchResults.ToList().IndexOf("Test Start"));
        Assert.Equal("Test Start", panel.FromText);
        Assert.Equal("Test Start", panel.FromInput);
        Assert.True(panel.FromSelectionVisible);
        Assert.False(panel.FromEditorVisible);
        panel.EditFromAction();
        Assert.True(panel.FromEditorVisible);

        panel.UpdateDestinationInputAction("Current");
        Assert.Contains("Current Location", panel.SearchResults);
        panel.SelectSearchResultAction(panel.SearchResults.ToList().IndexOf("Current Location"));
        Assert.Equal("Current Location", panel.DestinationInput);
        Assert.True(panel.DestinationSelectionVisible);
        Assert.False(panel.DestinationEditorVisible);
        panel.EditDestinationAction();
        Assert.True(panel.DestinationEditorVisible);
        host.PluginNavigation.SnapshotValue = host.PluginNavigation.SnapshotValue with
        {
            Position = new PluginNavigationPosition(0, 9, 9, 0, 0, true)
        };
        panel.StartNavigation();

        Assert.Equal("Current Location", plugin.CurrentDestinationName);
        Assert.Contains(new RouteFinding.Coordinates(9, 9).ToString(), plugin.DestinationPositionText());
        Assert.NotEmpty(plugin.GetCurrentRouteSteps());
        Assert.Empty(host.PluginNavigation.GoToPositionCalls);
        plugin.Disable();
    }

    [Fact]
    public void NamedFromPreviewsRouteEvenWhenAutoNavigateIsEnabled()
    {
        var host = new FakePluginHost { HasUiValue = false };
        new GoArrowSettings { AutoNavigate = true }.Save(host.PluginStorage);
        host.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(0, 40, 40, 0, 0, true), false, false);
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();
        GoArrowPanel panel = plugin.Panel!;

        panel.UpdateFromInputAction("Holt");
        panel.SelectSearchResultAction(panel.SearchResults.ToList().IndexOf("Holtburg"));
        panel.UpdateDestinationInputAction("Arwic");
        panel.SelectSearchResultAction(panel.SearchResults.ToList().IndexOf("Arwic"));
        panel.StartNavigation();

        Assert.Equal("Holtburg", panel.FromText);
        Assert.Equal("Arwic", plugin.CurrentDestinationName);
        Assert.NotEmpty(plugin.GetCurrentRouteSteps());
        Assert.Empty(host.PluginNavigation.GoToPositionCalls);
        Assert.Empty(host.PluginNavigation.GoToCalls);

        panel.UpdateDestinationInputAction("42.1N 33.6E");
        panel.SubmitDestination();
        panel.StartNavigation();
        Assert.Empty(host.PluginNavigation.GoToPositionCalls);
        Assert.Empty(host.PluginNavigation.GoToCalls);
        plugin.Disable();
    }

    [Fact]
    public void CurrentLocationInBothFieldsDoesNotStartMovement()
    {
        var host = new FakePluginHost { HasUiValue = false };
        new GoArrowSettings { AutoNavigate = true }.Save(host.PluginStorage);
        host.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(0, 42, 33, 0, 0, true), false, false);
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();
        GoArrowPanel panel = plugin.Panel!;

        panel.UpdateDestinationInputAction("Current Location");
        panel.SubmitDestination();
        panel.StartNavigation();

        Assert.Equal("Current Location", panel.FromInput);
        Assert.Equal("Current Location", plugin.CurrentDestinationName);
        Assert.Empty(host.PluginNavigation.GoToPositionCalls);
        Assert.Empty(host.PluginNavigation.GoToCalls);
        plugin.Disable();
    }

    [Fact]
    public void StartupLoadsCachedAtlasPortalRoutes()
    {
        var host = new FakePluginHost { HasUiValue = false };
        host.PluginStorage.WriteText("data/warcry-atlas.xml", """
            <atlas>
              <location><id>1</id><name>Start</name><type>Town</type><latitude>0</latitude><longitude>1</longitude><retired>N</retired></location>
              <location><id>2</id><name>Far Portal</name><type>Wilderness Portal</type><latitude>0</latitude><longitude>2</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>95</arrival_longitude><description>Take the portal near the town gate.</description><retired>N</retired></location>
              <location><id>3</id><name>End</name><type>Town</type><latitude>0</latitude><longitude>96</longitude><retired>N</retired></location>
            </atlas>
            """);
        host.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            IsAvailable: true,
            IsPortalSpace: false,
            LocalObjectId: 1,
            Position: new PluginNavigationPosition(0, 1, 0, 0, 0, true),
            IsMoving: false,
            IsAirborne: false);
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();

        Assert.True(plugin.SetDestination("End"));
        plugin.Go();

        Assert.Contains(plugin.GetCurrentRouteSteps(), step => step == "Portal: Far Portal");
        GoArrowPanel panel = plugin.Panel!;
        int portalIndex = panel.RouteSteps.ToList().IndexOf("Portal: Far Portal");
        panel.SelectRouteStepAction(portalIndex);
        Assert.True(panel.DetailsTabSelected);
        Assert.False(panel.RouteTabVisible);
        Assert.Equal(portalIndex, panel.SelectedRouteStep);
        Assert.Equal("Far Portal", panel.DetailsLocationName);
        Assert.Equal("0.0N, 2.0E", panel.DetailsCoordinates);
        Assert.True(panel.DetailsArrivalVisible);
        Assert.Equal("0.0N, 95.0E", panel.DetailsArrivalCoordinates);
        Assert.Contains(panel.DetailsNotesLines, line => line.Contains("town gate"));
        panel.ShowRouteTab();
        Assert.True(panel.RouteTabSelected);
        Assert.Equal(portalIndex, panel.SelectedRouteStep);
        Assert.Empty(host.PluginNavigation.GoToPositionCalls);
        plugin.Disable();
    }

    [Fact]
    public void RecalculationKeepsOutdoorRouteWhileInsideDungeon()
    {
        var host = new FakePluginHost { HasUiValue = false };
        host.PluginStorage.WriteText("data/warcry-atlas.xml", """
            <atlas>
              <location><id>1</id><name>Start</name><type>Town</type><latitude>0</latitude><longitude>1</longitude><retired>N</retired></location>
              <location><id>2</id><name>Far Portal</name><type>Wilderness Portal</type><latitude>0</latitude><longitude>2</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>95</arrival_longitude><retired>N</retired></location>
              <location><id>3</id><name>Other Dungeon</name><type>Dungeon</type><latitude>0</latitude><longitude>95</longitude><dungeon_id>1234</dungeon_id><retired>N</retired></location>
              <location><id>4</id><name>End</name><type>Town</type><latitude>0</latitude><longitude>96</longitude><retired>N</retired></location>
            </atlas>
            """);
        host.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(0, 1, 0, 0, 0, true), false, false);
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();
        Assert.True(plugin.SetDestination("End"));
        plugin.Go();
        string[] original = plugin.GetCurrentRouteSteps().ToArray();
        Assert.Contains(original, step => step == "Portal: Far Portal");

        host.PluginNavigation.SnapshotValue = host.PluginNavigation.SnapshotValue with
        {
            Position = new PluginNavigationPosition(0x12340100, 95, 0, 0, 0, false)
        };
        host.PluginEvents.RaiseTick(0.1);
        host.PluginEvents.RaiseTick(0.1);
        Assert.Equal(original, plugin.GetCurrentRouteSteps());
        Assert.Empty(host.PluginNavigation.GoToPositionCalls);
        Assert.Equal("Outdoor route paused indoors", plugin.Panel!.NavStatusText);
        Assert.Empty(plugin.Panel.BearingText);

        host.PluginNavigation.SnapshotValue = host.PluginNavigation.SnapshotValue with
        {
            Position = new PluginNavigationPosition(0, 95, 0, 0, 0, true)
        };
        host.PluginEvents.RaiseTick(0.1);
        Assert.DoesNotContain(plugin.GetCurrentRouteSteps(), step => step == "Portal: Far Portal");
        plugin.Disable();
    }

    [Fact]
    public void GoDoesNotPlanOutdoorRouteFromIndoorPosition()
    {
        var host = new FakePluginHost { HasUiValue = false };
        host.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(0x12340100, 95, 0, 0, 0, false), false, false);
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();
        Assert.True(plugin.SetDestination("Holtburg"));

        plugin.Go();

        Assert.Empty(plugin.GetCurrentRouteSteps());
        Assert.Empty(host.PluginNavigation.GoToPositionCalls);
        plugin.Disable();
    }

    [Fact]
    public void GoPlansRouteWithoutIssuingMovementWhenAutoNavigateIsOff()
    {
        var host = new FakePluginHost { HasUiValue = false };
        host.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            IsAvailable: true,
            IsPortalSpace: false,
            LocalObjectId: 1,
            Position: new PluginNavigationPosition(0, 0, 0, 0, 0, true),
            IsMoving: false,
            IsAirborne: false);
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();

        Assert.True(plugin.SetDestination("Holtburg"));
        plugin.Go();

        Assert.NotEmpty(plugin.GetCurrentRouteSteps());
        Assert.Empty(host.PluginNavigation.GoToPositionCalls);
        Assert.Empty(host.PluginNavigation.GoToCalls);
        plugin.Disable();
    }
}
