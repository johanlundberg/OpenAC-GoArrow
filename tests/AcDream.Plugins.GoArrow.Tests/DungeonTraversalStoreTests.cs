using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class DungeonTraversalStoreTests
{
    private static PluginNavigationPosition Point(double x, double y = 0, double z = 0) =>
        DungeonPortalSearchTests.Position((float)x) with
        { NorthSouth = DungeonPortalSearchTests.Position(0).NorthSouth + y / 240, Elevation = z / 240 };
    private static PluginNavigationSnapshot Snapshot(PluginNavigationPosition p) => new(true, false, 1, p, true, false);
    private static PluginWorldObject Portal(string name) => new(44, 0, name, PluginObjectClass.Portal, 0, 0, 0)
    { PortalDestination = name };

    [Fact]
    public void ShortestObservedRouteDropsLoopsAndKeepsMultipleExitsSeparateAfterReload()
    {
        var storage = new FakePluginStorage();
        var store = new DungeonTraversalStore(storage);
        foreach (var p in new[] { Point(0), Point(6), Point(6, 6), Point(0, 6), Point(0), Point(0, -6) })
            store.Observe(Snapshot(p));
        store.RememberExit("south", Portal("South exit"), Snapshot(Point(0, -6)));
        store.BreakTrace();
        store.Observe(Snapshot(Point(0)));
        store.Observe(Snapshot(Point(6)));
        store.RememberExit("east", Portal("East exit"), Snapshot(Point(6)));

        var reloaded = new DungeonTraversalStore(storage);
        reloaded.Load();
        var south = reloaded.FindPath(Point(0), "south");
        Assert.Equal(2, south.Count);
        Assert.Equal(Point(0, -6), south[^1]);
        var east = reloaded.FindPath(Point(0), "east");
        Assert.Equal(2, east.Count);
        Assert.Equal(Point(6), east[^1]);
        Assert.Empty(reloaded.FindPath(Point(0), "unlearned"));
        Assert.Empty(reloaded.FindPath(Point(0) with { Elevation = -12 / 240d }, "east"));
        Assert.Empty(reloaded.FindPath(Point(0) with { CellId = 0x02AB022F }, "east"));
        // Directions are observed separately; a cliff drop is not a climb.
        Assert.Empty(reloaded.FindPath(Point(0, -6), "east"));
    }

    [Fact]
    public void SharingMergesRecordsIdempotentlyAndRejectsInvalidImportsWithoutMutation()
    {
        var source = new DungeonTraversalStore(new FakePluginStorage());
        source.Observe(Snapshot(Point(0)));
        source.Observe(Snapshot(Point(6, 0, -6)));
        source.RememberExit("downstairs", Portal("Lower exit"), Snapshot(Point(6, 0, -6)));
        var target = new DungeonTraversalStore(new FakePluginStorage());
        target.Import(source.Export());
        string once = target.Export();
        target.Import(source.Export());
        Assert.Equal(once, target.Export());
        Assert.Equal(-6 / 240d, target.FindPath(Point(0), "downstairs")[^1].Elevation);
        Assert.Throws<ArgumentException>(() => target.Import("""
            {"Version":1,"Points":[],"Edges":[{"From":0,"To":1}],"Exits":[]}
            """));
        Assert.Equal(once, target.Export());
        Assert.Throws<ArgumentException>(() => target.Import("{\"Version\":2}"));
    }

    [Fact]
    public void PortalSpaceAndLargeMovementGapsDoNotCreateWalkableConnections()
    {
        var store = new DungeonTraversalStore(new FakePluginStorage());
        store.Observe(Snapshot(Point(0)));
        store.Observe(Snapshot(Point(0)) with { IsPortalSpace = true });
        store.Observe(Snapshot(Point(6)));
        store.RememberExit("teleport", Portal("Exit"), Snapshot(Point(6)));
        Assert.Empty(store.FindPath(Point(0), "teleport"));
        store.Observe(Snapshot(Point(40)));
        store.RememberExit("gap", Portal("Exit"), Snapshot(Point(40)));
        Assert.Empty(store.FindPath(Point(6), "gap"));
    }

    [Fact]
    public void SearchUsesRequestedRecordedExitAndFallsBackWhenItsPathFails()
    {
        var store = new DungeonTraversalStore(new FakePluginStorage());
        store.Observe(Snapshot(Point(0)));
        store.Observe(Snapshot(Point(6)));
        store.RememberExit("east", Portal("East exit"), Snapshot(Point(6)));
        var navigation = new DungeonSearchNavigation();
        var automation = new DungeonSearchAutomation(navigation);
        navigation.Inner.SnapshotValue = Snapshot(Point(0));
        automation.Map.Cells = [new(0x02AA0101, new System.Numerics.Vector3(20, 0, 0), 0)];
        var search = new DungeonPortalSearch(store);
        search.UseRecordedExit("east");
        Assert.True(search.TryGetNextTarget(automation, out var saved));
        Assert.Equal(Point(6), saved);
        Assert.Contains("Following recorded path", search.Reason);
        search.TargetAccepted(saved.CellId);
        search.CompleteTarget(Point(0), false);
        Assert.True(search.TryGetNextTarget(automation, out var fallback));
        Assert.Equal(0x02AA0101u, fallback.CellId);
        Assert.DoesNotContain("Following recorded path", search.Reason);
    }
}
