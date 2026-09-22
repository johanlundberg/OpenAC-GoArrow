using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class RoutePlanningIntegrationTests
{
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
