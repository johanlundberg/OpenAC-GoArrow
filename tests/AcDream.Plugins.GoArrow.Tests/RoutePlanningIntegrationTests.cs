using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class RoutePlanningIntegrationTests
{
    [Fact]
    public void StartupLoadsCachedAtlasPortalRoutes()
    {
        var host = new FakePluginHost { HasUiValue = false };
        host.PluginStorage.WriteText("data/warcry-atlas.xml", """
            <atlas>
              <location><id>1</id><name>Start</name><type>Town</type><latitude>0</latitude><longitude>1</longitude><retired>N</retired></location>
              <location><id>2</id><name>Far Portal</name><type>Wilderness Portal</type><latitude>0</latitude><longitude>2</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>95</arrival_longitude><retired>N</retired></location>
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

        Assert.Contains(plugin.GetCurrentRouteSteps(), step => step.Contains("Portal [Far Portal]"));
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
