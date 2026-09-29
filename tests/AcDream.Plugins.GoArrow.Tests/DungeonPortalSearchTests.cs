using System.Numerics;
using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class DungeonPortalSearchTests
{
    [Fact]
    public void RecordedWalkShortcutsClearFloorButKeepsWallPortalAndStoreyTurns()
    {
        static PluginNavigationPosition At(double x, double y = 0, double z = 0, uint cell = 0x02AA022F) =>
            DungeonPortalSearchTests.Position((float)x) with
            { NorthSouth = DungeonPortalSearchTests.Position(0).NorthSouth + y / 240,
              Elevation = z / 240, CellId = cell };
        IReadOnlyList<Vector2> floor = [new(-5, -10), new(50, -10), new(50, 10), new(-5, 10)];
        PluginDungeonFloorplan Plan(IReadOnlyList<PluginDungeonWall> walls) => new(0x02AA0000,
            [new(0, [floor], walls), new(-6, [floor], [])],
            [new(0x02AA022F, Vector3.Zero, 0), new(0x02AA0230, Vector3.Zero, -6),
             new(0x02AA0231, Vector3.Zero, 0)], Vector3.Zero, Vector3.One);
        PluginNavigationPosition[] wandering = [At(0), At(6, 4), At(12, 4), At(18, 4), At(24)];
        var direct = DungeonPortalSearch.SimplifyRecordedPath(wandering, Plan([]), []);
        Assert.Equal(new[] { At(0), At(24) }, direct);
        var walled = DungeonPortalSearch.SimplifyRecordedPath(wandering,
            Plan([new(new Vector2(12, -1), new Vector2(12, 1))]), []);
        Assert.True(walled.Count > 2);
        var portalPath = new[] { At(0), At(6, 8), At(12, 8), At(24) };
        var aroundPortal = DungeonPortalSearch.SimplifyRecordedPath(portalPath, Plan([]), [At(12, -3)]);
        Assert.True(aroundPortal.Count > 2);
        Assert.Equal(At(24), aroundPortal[^1]);
        var stairs = new[] { At(0), At(6, 0, -6, 0x02AA0230), At(12, 0, 0, 0x02AA0231) };
        Assert.Equal(stairs, DungeonPortalSearch.SimplifyRecordedPath(stairs, Plan([]), []));
    }

    [Theory]
    [InlineData(-6)]
    [InlineData(-12)]
    public void DownstairsTargetKeepsItsFloorHeightEvenWhenGeometryExtendsUpstairs(float floor)
    {
        var navigation = new DungeonSearchNavigation();
        var automation = new DungeonSearchAutomation(navigation);
        navigation.Inner.SnapshotValue = new(true, false, 1, Position(0), false, false);
        // Same horizontal location, but on a lower floor. Its geometry
        // centre is upstairs, so it cannot be used as the walking height.
        automation.Map.Cells = [new(0x02AA0101, new Vector3(0, 0, 0), floor)];
        var search = new DungeonPortalSearch();
        Assert.True(search.TryGetNextTarget(automation, out var downstairs));
        Assert.Equal(0x02AA0101u, downstairs.CellId);
        Assert.Equal(floor / 240d, downstairs.Elevation);
        search.TargetAccepted(downstairs.CellId);
        navigation.Inner.SnapshotValue = navigation.Inner.SnapshotValue with { Position = downstairs };
        search.CompleteTarget(downstairs, true);
        Assert.True(search.TryGetNextTarget(automation, out var upstairs));
        Assert.Contains("Backtracking", search.Reason);
        Assert.Equal(0, upstairs.Elevation);
    }

    [Fact]
    public void SearchFollowsDistantForwardTargetBeforeBacktrackingToNearbySideBranch()
    {
        var navigation = new DungeonSearchNavigation();
        var automation = new DungeonSearchAutomation(navigation);
        navigation.Inner.SnapshotValue = new(true, false, 1, Position(0), false, false);
        automation.Map.Cells = [new(0x02AA0101, new Vector3(20, 0, 0), 0),
            new(0x02AA0102, new Vector3(40, 0, 0), 0),
            new(0x02AA0103, new Vector3(30, 30, 0), 0),
            new(0x02AA0104, new Vector3(100, 0, 0), 0)];
        var search = new DungeonPortalSearch();

        void Arrive(uint expected, bool backtracking = false)
        {
            Assert.True(search.TryGetNextTarget(automation, out var target));
            Assert.Equal(expected, target.CellId);
            Assert.Equal(backtracking, search.Reason.StartsWith("Backtracking"));
            search.TargetAccepted(target.CellId);
            var arrived = target with { Elevation = 0 };
            navigation.Inner.SnapshotValue = navigation.Inner.SnapshotValue with { Position = arrived };
            search.CompleteTarget(arrived, true);
        }

        Arrive(0x02AA0101);
        Arrive(0x02AA0102);
        Arrive(0x02AA0104);
        Arrive(0x02AA0102, true);
        Arrive(0x02AA0101, true);
        Arrive(0x02AA0103);
        Arrive(0x02AA0101, true);
        Arrive(0x02AA022F, true);
        Assert.False(search.TryGetNextTarget(automation, out _));
        Assert.Empty(navigation.PreviewTargets);
    }

    [Fact]
    public void FailedTargetDoesNotBecomeABacktrackingCheckpoint()
    {
        var navigation = new DungeonSearchNavigation();
        var automation = new DungeonSearchAutomation(navigation);
        navigation.Inner.SnapshotValue = new(true, false, 1, Position(0), false, false);
        automation.Map.Cells = [new(0x02AA0101, new Vector3(20, 0, 0), 0),
            new(0x02AA0102, new Vector3(100, 0, 0), 0)];
        var search = new DungeonPortalSearch();
        Assert.True(search.TryGetNextTarget(automation, out var failed));
        search.TargetAccepted(failed.CellId);
        search.CompleteTarget(navigation.Snapshot.Position, false);
        Assert.True(search.TryGetNextTarget(automation, out var next));
        Assert.Equal(0x02AA0102u, next.CellId);
        Assert.DoesNotContain("Backtracking", search.Reason);
    }

    [Fact]
    public void TallCellBesideSurfacePortalIsExcludedAndCrossingPathIsRejected()
    {
        var navigation = new DungeonSearchNavigation();
        var automation = new DungeonSearchAutomation(navigation);
        navigation.Inner.SnapshotValue = new(true, false, 1, Position(0), false, false);
        automation.Inner.Objects = new SearchPortals([new PluginWorldObject(43, 0, "Surface Portal",
            PluginObjectClass.Portal, 0, 0, 0) { HasPosition = true, Position = Position(20) }]);
        automation.Map.Cells = [new(0x02AA0230, new Vector3(20, 0, 30), 0),
            new(0x02AA0232, new Vector3(50, 0, 0), 0),
            new(0x02AA0233, new Vector3(50, 30, 0), 0)];
        var search = new DungeonPortalSearch();
        Assert.False(search.TryGetNextTarget(automation, out _));
        Assert.Equal(0x02AA0232u, Assert.Single(navigation.PreviewTargets).CellId);
        Assert.True(search.TryGetNextTarget(automation, out var safe));
        Assert.Equal(0x02AA0233u, safe.CellId);
        Assert.DoesNotContain(navigation.PreviewTargets, p => p.CellId == 0x02AA0230);
        Assert.Empty(navigation.Inner.GoToPositionCalls);
        navigation.Inner.SnapshotValue = navigation.Inner.SnapshotValue with { Position = Position(13) };
        Assert.False(search.SafeToContinue(automation));
        Assert.Contains("Surface Portal", search.Reason);
    }

    [Fact]
    public void LeavingPortalIsAllowedButRunningAcrossItToFarEndpointIsNot()
    {
        Assert.True(DungeonPortalSearch.CrossesPortal([Position(0), Position(30)], [Position(5)]));
        Assert.False(DungeonPortalSearch.CrossesPortal([Position(0), Position(-20)], [Position(5)]));
        Assert.False(DungeonPortalSearch.CrossesPortal(
            [Position(0) with { Elevation = 12 / 240d }, Position(30) with { Elevation = 12 / 240d }], [Position(5)]));
    }

    [Fact]
    public void VisibleRoomCellsAreSkippedButCellsBehindWallsAndOnOtherFloorsRemain()
    {
        var navigation = new DungeonSearchNavigation();
        var automation = new DungeonSearchAutomation(navigation);
        navigation.Inner.SnapshotValue = new(true, false, 1, Position(0), false, false);
        automation.Map.Cells = [
            new(0x02AA0101, new Vector3(12, 0, 0), 0),
            new(0x02AA0102, new Vector3(16, 0, 0), 0),
            new(0x02AA0103, new Vector3(30, 5, 0), 0),
            new(0x02AA0104, new Vector3(50, 0, 0), 0), // outside every floor polygon
            new(0x02AA0105, new Vector3(30, 0, 12), 12),
        ];
        IReadOnlyList<Vector2> floor = [new(-5, -10), new(40, -10), new(40, 10), new(-5, 10)];
        automation.Map.Layers = [new(0, [floor], [new(new Vector2(20, 2), new Vector2(20, 10))]),
            new(12, [floor], [])];
        var search = new DungeonPortalSearch();
        Assert.True(search.TryGetNextTarget(automation, out var first));
        Assert.Equal(0x02AA0103u, first.CellId);
        search.TargetAccepted(first.CellId);
        Assert.True(search.TryGetNextTarget(automation, out var second));
        Assert.Equal(0x02AA0105u, second.CellId);
        search.TargetAccepted(second.CellId);
        Assert.Empty(navigation.PreviewTargets);
        Assert.False(search.TryGetNextTarget(automation, out _));
    }

    [Fact]
    public void SearchVisitsNextRoomOnceInsteadOfWalkingThroughItsCells()
    {
        var navigation = new DungeonSearchNavigation();
        var automation = new DungeonSearchAutomation(navigation);
        navigation.Inner.SnapshotValue = new(true, false, 1, Position(0), false, false);
        automation.Map.Cells = [
            new(0x02AA0101, new Vector3(12, 0, 0), 0),
            new(0x02AA0102, new Vector3(16, 0, 0), 0),
            new(0x02AA0103, new Vector3(30, 5, 0), 0),
            new(0x02AA0104, new Vector3(32, 7, 0), 0),
        ];
        IReadOnlyList<Vector2> floor = [new(-5, -10), new(40, -10), new(40, 10), new(-5, 10)];
        automation.Map.Layers = [new(0, [floor],
            [new(new Vector2(20, -10), new Vector2(20, 10))])];
        var search = new DungeonPortalSearch();

        Assert.True(search.TryGetNextTarget(automation, out var nextRoom));
        Assert.Equal(0x02AA0103u, nextRoom.CellId);
        search.TargetAccepted(nextRoom.CellId);
        navigation.Inner.SnapshotValue = navigation.Inner.SnapshotValue with { Position = nextRoom };
        search.CompleteTarget(nextRoom, true);

        Assert.True(search.TryGetNextTarget(automation, out var returnTarget));
        Assert.Equal(Position(0).CellId, returnTarget.CellId);
        Assert.Contains("Backtracking", search.Reason);
        search.TargetAccepted(returnTarget.CellId);
        navigation.Inner.SnapshotValue = navigation.Inner.SnapshotValue with { Position = Position(0) };
        search.CompleteTarget(Position(0), true);
        Assert.False(search.TryGetNextTarget(automation, out _));
    }

    [Theory]
    [InlineData(1, false, false)]
    [InlineData(2, false, false)]
    [InlineData(4, false, true)]
    [InlineData(2, true, true)]
    public void SearchSkipsShallowDeadEndButKeepsLongPassageAndPortals(
        int hallwayCells, bool portalInHallway, bool shouldExplore)
    {
        var navigation = new DungeonSearchNavigation();
        var automation = new DungeonSearchAutomation(navigation);
        navigation.Inner.SnapshotValue = new(true, false, 1,
            Position(0) with { NorthSouth = Position(0).NorthSouth + 10 / 240d }, false, false);
        var cells = new List<PluginDungeonCell>
        {
            new(0x02AA0101, new Vector3(15, 0, 0), 0),
            new(0x02AA0102, new Vector3(26, 0, 0), 0),
        };
        for (int i = 1; i < hallwayCells; i++)
            cells.Add(new(0x02AA0102u + (uint)i, new Vector3(26 + i * 12, 0, 0), 0));
        automation.Map.Cells = cells;
        if (portalInHallway)
            automation.Inner.Objects = new SearchPortals([new PluginWorldObject(43, 0, "Dungeon Portal",
                PluginObjectClass.Portal, 0, 0, 0)
                { HasPosition = true, Position = Position(50) }]);
        int end = 26 + (hallwayCells - 1) * 12 + 7;
        IReadOnlyList<Vector2> floor = [new(-5, -12), new(80, -12), new(80, 15), new(-5, 15)];
        automation.Map.Layers = [new(0, [floor],
            [new(new Vector2(20, -12), new Vector2(20, -2)),
             new(new Vector2(20, 2), new Vector2(20, 15)),
             new(new Vector2(20, -5), new Vector2(end, -5)),
             new(new Vector2(20, 5), new Vector2(end, 5)),
             new(new Vector2(end, -5), new Vector2(end, 5))])];
        var search = new DungeonPortalSearch();

        Assert.Equal(shouldExplore, search.TryGetNextTarget(automation, out var target));
        if (shouldExplore)
            Assert.Equal(0x02AA0102u, target.CellId);
    }

    [Fact]
    public void SearchUsesWalkPlannerWithoutPreviewAndDoesNotConsumeRefusedTargets()
    {
        var navigation = new DungeonSearchNavigation();
        var automation = new DungeonSearchAutomation(navigation);
        navigation.Inner.SnapshotValue = new(true, false, 1, Position(0), false, false);
        automation.Map.Cells = [new(0x02AA0101, new Vector3(20, 0, 0), 0),
            new(0x02AA0102, new Vector3(50, 0, 0), 0)];
        var search = new DungeonPortalSearch();
        Assert.True(search.TryGetNextTarget(automation, out var first));
        Assert.True(search.TryGetNextTarget(automation, out var refused));
        Assert.Equal(first.CellId, refused.CellId);
        search.TargetAccepted(first.CellId);
        search.RecordFailure("No path to the first cell");
        Assert.True(search.TryGetNextTarget(automation, out var second));
        Assert.Equal(0x02AA0102u, second.CellId);
        Assert.Equal(0, second.Elevation);
        search.TargetAccepted(second.CellId);
        Assert.False(search.TryGetNextTarget(automation, out _));
        Assert.Contains("No path to the first cell", search.Reason);
        Assert.Contains("2 attempted", search.Reason);
        search.Reset();
        Assert.True(search.TryGetNextTarget(automation, out var restarted));
        Assert.Equal(first.CellId, restarted.CellId);
        Assert.Empty(navigation.PreviewTargets);
    }

    internal static PluginNavigationPosition Position(float x) => new(
        0x02AA022F, (-125 * 192 + x - 84) / 240d, (43 * 192 - 84) / 240d, 0, 0, false);
}

internal sealed class DungeonSearchNavigation : INavigationAutomation
{
    public FakeNavigationAutomation Inner { get; } = new();
    public List<PluginNavigationPosition> PreviewTargets { get; } = [];
    public Func<PluginNavigationPosition, Task<PluginNavigationPlan>> Plan { get; set; } =
        p => Task.FromResult(new PluginNavigationPlan(PluginNavigationPlanStatus.Routed,
            [DungeonPortalSearchTests.Position(0), p], ""));
    public PluginNavigationSnapshot Snapshot => Inner.Snapshot;
    public PluginGoToReport GoToReport => Inner.GoToReport;
    public bool TryGetObject(uint id, out PluginNavigationObject obj) => Inner.TryGetObject(id, out obj);
    public PluginNavigationCommandStatus SetMovementIntent(in PluginMovementIntent intent) => Inner.SetMovementIntent(intent);
    public PluginNavigationCommandStatus ClearMovementIntent() => Inner.ClearMovementIntent();
    public Task<PluginNavigationPlan> PreviewPathAsync(PluginNavigationPosition p, float arrivalMeters = 2.5f)
    { PreviewTargets.Add(p); return Plan(p); }
    public PluginNavigationCommandStatus GoTo(PluginNavigationPosition p, float arrivalMeters) => Inner.GoTo(p, arrivalMeters);
    public PluginNavigationCommandStatus GoTo(uint id, float arrivalMeters) => Inner.GoTo(id, arrivalMeters);
    public PluginNavigationCommandStatus StopGoTo() => Inner.StopGoTo();
}

internal sealed class DungeonSearchAutomation(DungeonSearchNavigation navigation) : IAutomationSurface
{
    public FakeAutomationSurface Inner { get; } = new();
    public DungeonSearchMap Map { get; } = new();
    public bool IsAvailable => true;
    public ICharacterInfo Character => Inner.Character;
    public ISpellCatalog Spells => Inner.Spells;
    public IMagicCommands Magic => Inner.Magic;
    public IPluginChat Chat => Inner.Chat;
    public INavigationAutomation Navigation => navigation;
    public IWorldObjectAutomation Objects => Inner.Objects;
    public IDungeonMapAutomation DungeonMap => Map;
}
internal sealed class DungeonSearchMap : IDungeonMapAutomation
{
    public IReadOnlyList<PluginDungeonCell> Cells { get; set; } = [];
    public IReadOnlyList<PluginDungeonLayer> Layers { get; set; } = [];
    public bool SealedDungeon { get; set; }
    public bool IsSealedDungeon(uint cellId) => SealedDungeon;
    public PluginDungeonFloorplan CaptureFloorplan(uint block) => new(block, Layers, Cells, Vector3.Zero, Vector3.One);
}

internal sealed class SearchPortals(IReadOnlyList<PluginWorldObject> portals) : IWorldObjectAutomation
{
    public IReadOnlyList<PluginWorldObject> CaptureObjects() => portals;
}
