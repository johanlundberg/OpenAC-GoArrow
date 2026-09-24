using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class IndoorPathPreviewTests
{
    [Fact]
    public void ShowsLengthAndCellAwareWaypointsWithoutStartingWalk()
    {
        var host = HostAt(0x12340100);
        var (settings, destination, navigator) = IndoorDestination(host);
        using (navigator)
        {
            var pending = new TaskCompletionSource<PluginNavigationPlan>(
                TaskCreationOptions.RunContinuationsAsynchronously
            );
            var calls = new List<PluginNavigationPosition>();
            var preview = new GoArrowIndoorPathPreview(
                host,
                navigator,
                settings,
                previewPosition: (position, _) =>
                {
                    calls.Add(position);
                    return pending.Task;
                }
            );
            var panel = new GoArrowPanel(host, new GoArrowPlugin(), settings, destination, navigator, preview);

            preview.OnTick();
            Assert.Single(calls);
            Assert.Equal((uint)0x12340122, calls[0].CellId);
            Assert.Equal("Planning indoor path...", panel.IndoorPathSummary);
            Assert.True(panel.IndoorPathVisible);
            Assert.False(panel.RouteListVisible);
            Assert.Empty(host.PluginNavigation.GoToPositionCalls);

            pending.SetResult(
                new PluginNavigationPlan(
                    PluginNavigationPlanStatus.Routed,
                    [
                        host.PluginNavigation.Snapshot.Position,
                        new PluginNavigationPosition(0x12340122, 10.01, 10.02, 0.025, 0, false),
                    ],
                    string.Empty
                ) { LengthMeters = 37.5f }
            );
            preview.OnTick();

            Assert.Equal("Indoor path: 37.5 m, 2 waypoints", panel.IndoorPathSummary);
            Assert.Equal(2, panel.IndoorPathWaypoints.Count);
            Assert.Contains("Cell 0x12340122", panel.IndoorPathWaypoints[1]);
            Assert.Contains("Z 6 m", panel.IndoorPathWaypoints[1]);
            Assert.Empty(host.PluginNavigation.GoToCalls);
            Assert.Empty(host.PluginNavigation.GoToPositionCalls);
        }
    }

    [Fact]
    public void IgnoresOldPlanAfterTargetChangeAndClearsOnDungeonExit()
    {
        var host = HostAt(0x12340100);
        var (settings, destination, navigator) = IndoorDestination(host);
        using (navigator)
        {
            var first = new TaskCompletionSource<PluginNavigationPlan>();
            var second = new TaskCompletionSource<PluginNavigationPlan>();
            int calls = 0;
            var preview = new GoArrowIndoorPathPreview(
                host,
                navigator,
                settings,
                previewPosition: (_, _) => ++calls == 1 ? first.Task : second.Task
            );

            preview.OnTick();
            Assert.True(destination.SetDestination("Second Chamber"));
            preview.OnTick();
            Assert.Equal(2, calls);
            first.SetResult(Routed(11));
            preview.OnTick();
            Assert.Equal("Planning indoor path...", preview.Summary);

            second.SetResult(Routed(52));
            preview.OnTick();
            Assert.StartsWith("Indoor path: 52 m", preview.Summary);

            host.PluginNavigation.SnapshotValue = host.PluginNavigation.SnapshotValue with
            {
                Position = host.PluginNavigation.Snapshot.Position with { IsOutdoor = true },
            };
            preview.OnTick();
            Assert.False(preview.Visible);
            Assert.Empty(preview.Waypoints);
        }
    }

    [Fact]
    public void SelectedIndoorObjectUsesObjectPreview()
    {
        var host = HostAt(0x12340100);
        var (settings, destination, navigator) = IndoorDestination(host);
        using (navigator)
        {
            var selected = new PluginWorldObject(
                42, 0, "Chest", PluginObjectClass.Npc, 0, 0, 0
            )
            {
                HasPosition = true,
                Position = new PluginNavigationPosition(0x12340122, 10.01, 10.01, 0, 0, false),
            };
            Assert.True(destination.SetObject(selected));
            uint requestedId = 0;
            int calls = 0;
            var preview = new GoArrowIndoorPathPreview(
                host,
                navigator,
                settings,
                previewObject: (id, _) =>
                {
                    requestedId = id;
                    calls++;
                    return Task.FromResult(Routed(12));
                }
            );

            preview.OnTick();
            preview.OnTick();

            Assert.Equal((uint)42, requestedId);
            Assert.StartsWith("Indoor path: 12 m", preview.Summary);
            destination.UpdateObject(selected with
            {
                Position = selected.Position with { EastWest = 10.011, HeadingDegrees = 90 },
            });
            preview.OnTick();
            Assert.Equal(1, calls);
        }
    }

    [Fact]
    public void NamedLivePortalCanBePreviewedWithoutChangingNavigatorState()
    {
        var host = HostAt(0x12340100);
        var objects = new PortalObjects();
        objects.Items.Add(new PluginWorldObject(
            71, 0, "Portal to Sawato", PluginObjectClass.Portal, 0, 0, 0
        )
        {
            HasPosition = true,
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            Position = new PluginNavigationPosition(0x12340122, 10.01, 10, 0, 0, false),
        });
        host.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.PluginNavigation,
            Chat = host.PluginChat,
            Objects = objects,
        };
        var settings = new GoArrowSettings();
        var database = new LocationDatabase();
        database.LoadLocationsXml(
            "<locations><loc name='Sawato Portal' type='TownPortal' NS='-28.7' EW='59.3' /></locations>"
        );
        var destination = new GoArrowDestination(settings, database, new RouteFinder(database));
        Assert.True(destination.SetDestination("Sawato Portal"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        uint requestedId = 0;
        var preview = new GoArrowIndoorPathPreview(
            host,
            navigator,
            settings,
            previewObject: (id, _) =>
            {
                requestedId = id;
                return Task.FromResult(Routed(45));
            }
        );

        preview.OnTick();
        preview.OnTick();

        Assert.Equal((uint)71, requestedId);
        Assert.StartsWith("Indoor path: 45 m", preview.Summary);
        Assert.False(navigator.HasIndoorTarget);
        Assert.Empty(host.PluginNavigation.GoToCalls);
    }

    private static PluginNavigationPlan Routed(float length) =>
        new(PluginNavigationPlanStatus.Routed, [], string.Empty) { LengthMeters = length };

    private static FakePluginHost HostAt(uint cellId)
    {
        var host = new FakePluginHost { HasUiValue = true };
        host.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true,
            false,
            1,
            new PluginNavigationPosition(cellId, 10, 10, 0, 0, false),
            false,
            false
        );
        return host;
    }

    private static (GoArrowSettings, GoArrowDestination, GoArrowNavigator) IndoorDestination(
        FakePluginHost host
    )
    {
        var settings = new GoArrowSettings();
        var database = new LocationDatabase();
        database.LoadLocationsXml(
            """
            <locations>
              <loc name="First Chamber" type="Dungeon" cellId="0x12340122" x="35" y="50" z="6" />
              <loc name="Second Chamber" type="Dungeon" cellId="0x12340123" x="55" y="50" z="6" />
            </locations>
            """
        );
        var destination = new GoArrowDestination(settings, database, new RouteFinder(database));
        Assert.True(destination.SetDestination("First Chamber"));
        return (settings, destination, new GoArrowNavigator(host, destination, settings));
    }

    private sealed class PortalObjects : IWorldObjectAutomation
    {
        public List<PluginWorldObject> Items { get; } = [];
        public IReadOnlyList<PluginWorldObject> CaptureObjects() => Items;
    }
}
