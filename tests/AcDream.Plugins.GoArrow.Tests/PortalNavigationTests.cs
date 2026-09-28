using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class PortalNavigationTests
{
    [Fact]
    public void RouteButtonsCycleChoicesAndGoKeepsTheSelectedPortal()
    {
        var host = new NavigationHost();
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(true, false, 1,
            new PluginNavigationPosition(0, 0, 0, 0, 0, true), false, false);
        var db = new LocationDatabase();
        db.LoadLocationsXml("""
            <locations>
              <loc name="Portal A" type="WildernessPortal" NS="0" EW="1" exitNS="0" exitEW="99" />
              <loc name="Portal B" type="WildernessPortal" NS="0" EW="2" exitNS="0" exitEW="98" />
              <loc name="Portal C" type="WildernessPortal" NS="0" EW="3" exitNS="0" exitEW="97" />
              <loc name="End" type="Town" NS="0" EW="100" />
            </locations>
            """);
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        destination.CalculateRoute(new Location("Current Position", 0, 0));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        var origin = new Location("Current Position", 0, 0);
        Assert.True(destination.CycleAlternative(1, origin));
        Assert.Equal("Portal B", destination.CurrentRoute!.Steps.First(s => s.Kind == RouteStepKind.Portal).Via);
        Assert.True(destination.CycleAlternative(1, origin));
        Assert.False(destination.CycleAlternative(1, origin));
        Assert.True(destination.CycleAlternative(-1, origin));
        Assert.Equal(1, destination.AlternativeIndex);
        var panel = new GoArrowPanel(host, new GoArrowPlugin(), settings, destination, navigator);
        Assert.True(panel.CanPreviousRoute);
        Assert.True(panel.CanNextRoute);
        Assert.Contains("2/3", panel.RouteStepsText);

        navigator.StartNavigation();
        Assert.Equal("Portal B", destination.CurrentRoute!.Steps.First(s => s.Kind == RouteStepKind.Portal).Via);
        Assert.Equal(2, Assert.Single(host.Inner.PluginNavigation.GoToPositionCalls).Position.EastWest);
        Assert.False(panel.CanNextRoute);
    }

    [Fact]
    public void OutdoorNoProgressTriesSideDetoursThenReplansFromReachedPoint()
    {
        var host = new NavigationHost();
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(true, false, 1,
            new PluginNavigationPosition(0, -52.15, -62.55, 0, 0, true), false, false);
        var db = new LocationDatabase();
        db.LoadLocationsXml("<locations><loc name='Hurnmel the Smith' type='Vendor' NS='-65.4' EW='-44' /></locations>");
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("Hurnmel the Smith"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();
        var commands = host.Inner.PluginNavigation.GoToPositionCalls;
        Assert.Single(commands);
        host.EventsValue.RaiseNavigationChanged(new PluginGoToReport(1, PluginGoToState.NoRoute,
            0, 2000, 0, "no clear path leads any nearer the goal") { Revision = 1 });
        Assert.Equal(2, commands.Count);
        Assert.InRange(host.Inner.PluginNavigation.Snapshot.Position.HorizontalDistanceMeters(commands[1].Position), 79, 81);
        Assert.Contains("detour 1/12", navigator.FailureReason);
        host.EventsValue.RaiseNavigationChanged(new PluginGoToReport(2, PluginGoToState.NoRoute,
            0, 2000, 0, "no clear path leads any nearer the goal") { Revision = 2 });
        Assert.Equal(3, commands.Count);
        Assert.True(commands[1].Position.HorizontalDistanceMeters(commands[2].Position) > 100);
        host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
        { Position = commands[2].Position };
        host.EventsValue.RaiseNavigationChanged(new PluginGoToReport(3, PluginGoToState.Arrived,
            0, 0, 0, null) { Revision = 3 });
        Assert.Equal(4, commands.Count);
        Assert.Equal(-44, commands[3].Position.EastWest);
        Assert.Equal(-65.4, commands[3].Position.NorthSouth);
    }

    [Fact]
    public void UnrelatedOutdoorNoRouteDoesNotStartDetour()
    {
        var host = new NavigationHost();
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(true, false, 1,
            new PluginNavigationPosition(0, -52.15, -62.55, 0, 0, true), false, false);
        var db = new LocationDatabase();
        db.LoadLocationsXml("<locations><loc name='End' type='Town' NS='-65.4' EW='-44' /></locations>");
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();
        host.EventsValue.RaiseNavigationChanged(new PluginGoToReport(1, PluginGoToState.NoRoute,
            0, 2000, 0, "the destination is unavailable") { Revision = 1 });
        Assert.Single(host.Inner.PluginNavigation.GoToPositionCalls);
        Assert.False(navigator.IsNavigating);
    }

    [Fact]
    public void OutdoorDetoursStopAfterFiniteUnreachableCandidates()
    {
        var host = new NavigationHost();
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(true, false, 1,
            new PluginNavigationPosition(0, -52.15, -62.55, 0, 0, true), false, false);
        var db = new LocationDatabase();
        db.LoadLocationsXml("<locations><loc name='End' type='Town' NS='-65.4' EW='-44' /></locations>");
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();
        for (int sequence = 1; sequence <= 13 && navigator.IsNavigating; sequence++)
            host.EventsValue.RaiseNavigationChanged(new PluginGoToReport(sequence, PluginGoToState.NoRoute,
                0, 2000, 0, "no clear path leads any nearer the goal") { Revision = sequence });
        Assert.False(navigator.IsNavigating);
        Assert.InRange(host.Inner.PluginNavigation.GoToPositionCalls.Count, 2, 13);
        Assert.Contains("outdoor detour attempts", navigator.FailureReason);
    }

    [Theory]
    [InlineData(false, false, false)]
    [InlineData(true, false, false)]
    [InlineData(false, true, false)]
    [InlineData(true, true, false)]
    [InlineData(true, true, true)]
    public void WalkingIntoTownNetworkBeforeArrivalCompletesApproachAndCrossing(bool liveApproach, bool alreadyTeleported, bool resumeAfterStop)
    {
        var host = new NavigationHost();
        var objects = new TestWorldObjects();
        objects.Objects.Add(new PluginWorldObject(42, 0, "Portal to Town Network", PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true, Position = new PluginNavigationPosition(0x12340001, 2, 0, 0, 0, true),
        });
        objects.Objects.Add(new PluginWorldObject(43, 0, "Sawato Portal", PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true, Position = new PluginNavigationPosition(0x56780101, 95, 0, 0, 0, false),
        });
        host.Inner.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.Inner.PluginNavigation, Chat = host.Inner.PluginChat, Objects = objects,
        };
        host.Inner.PluginNavigation.SnapshotValue = new(true, false, 1,
            new PluginNavigationPosition(0x12340001, liveApproach ? 1.9 : 0, 0, 0, 0, true), false, false);
        var db = new LocationDatabase();
        db.LoadLocationsXml("""
            <locations>
              <loc name="Town Network Portal(Sanamar)" type="PortalHub" NS="0" EW="2" exitNS="0" exitEW="95" />
              <loc name="Town Network (S L 1) to Sawato" type="PortalHub" NS="0" EW="95" exitNS="0" exitEW="200" />
              <loc name="End" type="Vendor" NS="0" EW="201" />
            </locations>
            """);
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();
        long approachSequence = host.Inner.PluginNavigation.GoToReport.Sequence;
        var inside = new PluginNavigationPosition(0x56780100, 95, 0, 0, 0, false);
        host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
        { IsPortalSpace = !alreadyTeleported, Position = alreadyTeleported ? inside : host.Inner.PluginNavigation.Snapshot.Position };
        var lost = new PluginGoToReport(approachSequence, PluginGoToState.Lost,
            liveApproach ? 42u : 0u, 0, 0, "the character entered portal space") { Revision = 1 };
        if (resumeAfterStop)
        {
            navigator.StopNavigation();
            host.Inner.PluginNavigation.GoToReportValue = lost;
            navigator.ResumeNavigation();
        }
        else
            host.EventsValue.RaiseNavigationChanged(lost);
        if (!alreadyTeleported)
        {
            Assert.False(navigator.IsNavigating);
            host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
            { IsPortalSpace = false, Position = inside };
            navigator.OnTick(0.1);
        }
        Assert.True(navigator.IsNavigating);
        Assert.Equal("Town Network (S L 1) to Sawato", destination.CurrentRoute!.Steps[0].To.Name);
        Assert.Equal((uint)43, host.Inner.PluginNavigation.GoToCalls[^1].ObjectId);
        Assert.Empty(objects.Activated);
        // A delayed report from the entrance cannot stop the outgoing walk.
        host.EventsValue.RaiseNavigationChanged(new PluginGoToReport(
            approachSequence, PluginGoToState.Lost, 0, 0, 0, "the character entered portal space") { Revision = 2 });
        Assert.True(navigator.IsNavigating);
    }

    [Theory]
    [InlineData(false, false)]
    [InlineData(true, false)]
    [InlineData(false, true)]
    public void RestartInsideDesertMarchExploresThenSwitchesToBlackHillWithoutUsingSurfacePortal(bool reachExplorationCell, bool unreachableCell)
    {
        var host = new NavigationHost();
        var navigation = new DungeonSearchNavigation();
        var automation = new DungeonSearchAutomation(navigation);
        var objects = new TestWorldObjects();
        objects.Objects.Add(new PluginWorldObject(43, 0, "Surface Portal", PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = DungeonPortalSearchTests.Position(-5),
        });
        automation.Inner.Objects = objects;
        host.Inner.AutomationValue = automation;
        navigation.Inner.SnapshotValue = new(true, false, 1, DungeonPortalSearchTests.Position(0), false, false);
        automation.Map.Cells = [new(0x02AA0230, new System.Numerics.Vector3(20, 0, 0), 0),
            new(0x02AA0232, new System.Numerics.Vector3(50, 0, 0), 0)];
        var db = new LocationDatabase();
        db.LoadLocationsXml("""
            <locations>
              <loc name="Desert March" type="PortalHub" NS="0" EW="1" dungeonId="02AA" />
              <loc name="Desert March to Black Hill Portal" type="UndergroundPortal" NS="0" EW="1" exitNS="0" exitEW="95" />
              <loc name="End" type="Vendor" NS="0" EW="98" />
            </locations>
            """);
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();
        Assert.Equal(new uint[] { 43 }, objects.Identified);
        Assert.Empty(objects.Activated);
        navigator.OnTick(0.6);
        navigator.OnTick(0.6);
        Assert.Single(navigation.Inner.GoToPositionCalls);
        Assert.Contains("Exploring dungeon", navigator.FailureReason);
        Assert.Equal(1, destination.CurrentRoute!.StepCount);
        var panel = new GoArrowPanel(host, new GoArrowPlugin(), settings, destination, navigator);
        Assert.Contains(panel.RouteSteps, row => row.Contains("Dungeon Searching cell"));
        Assert.Equal(2, panel.RouteSteps.Count);
        panel.SelectRouteStepAction(1);
        Assert.Contains("Dungeon waypoint", panel.DetailsStepNumberText);
        Assert.Contains("floor", panel.DetailsCoordinates);
        if (reachExplorationCell || unreachableCell)
        {
            host.EventsValue.RaiseNavigationChanged(new PluginGoToReport(
                navigation.GoToReport.Sequence, unreachableCell ? PluginGoToState.NoRoute : PluginGoToState.Arrived,
                0, 0, 0, unreachableCell ? "cell is unreachable" : null) { Revision = 1 });
            Assert.Equal(1, destination.CurrentRoute.StepCount);
            Assert.False(navigator.HasArrived);
        }
        if (unreachableCell)
        {
            navigator.OnTick(0.6);
            Assert.Equal(2, navigation.Inner.GoToPositionCalls.Count);
            Assert.Equal(0x02AA0232u, navigation.Inner.GoToPositionCalls[^1].Position.CellId);
        }
        objects.Objects.Add(new PluginWorldObject(44, 0, "Black Hill", PluginObjectClass.Portal, 0, 0, 0)
        {
            PortalDestination = "Black Hill",
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = DungeonPortalSearchTests.Position(60),
        });
        navigator.OnTick(0.6);
        Assert.Equal((uint)44, Assert.Single(navigation.Inner.GoToCalls).ObjectId);
        Assert.Empty(objects.Activated);
        host.EventsValue.RaiseNavigationChanged(new PluginGoToReport(
            navigation.GoToReport.Sequence, PluginGoToState.Arrived, 44, 0, 0, null) { Revision = 2 });
        Assert.Equal(new uint[] { 44 }, objects.Activated);
    }

    [Fact]
    public void DungeonExitIdentifiesNamedPortalBeforeOtherVisiblePortals()
    {
        var host = new NavigationHost();
        var objects = new TestWorldObjects();
        objects.Objects.Add(new PluginWorldObject(42, 0, "Desert March to Bandit Castle Portal", PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(0, -28.8, -22.9, 0, 0, true),
        });
        objects.Objects.Add(new PluginWorldObject(43, 0, "Surface Portal", PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(0x02AA0101, 49.3, -65, 0, 0, false),
        });
        objects.Objects.Add(new PluginWorldObject(44, 0, "Bandit Castle Portal", PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(0x02AA0124, 49.4, -65, 0, 0, false),
        });
        host.Inner.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.Inner.PluginNavigation, Chat = host.Inner.PluginChat, Objects = objects,
        };
        host.Inner.PluginNavigation.SnapshotValue = new(true, false, 1,
            new PluginNavigationPosition(0, -28.8, -22.9, 0, 0, true), false, false);
        var db = new LocationDatabase();
        db.LoadLocationsXml("""
            <locations>
              <loc name="Desert March to Bandit Castle Portal" type="UndergroundPortal"
                   NS="-22.9" EW="-28.8" exitNS="-65" exitEW="49.3" />
              <loc name="Bandit Castle Lifestone" type="Lifestone" NS="-65.4" EW="49" />
            </locations>
            """);
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("Bandit Castle Lifestone"));
        destination.CalculateRoute(new Location("Current Position", -22.9, -28.8));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();
        host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
        {
            Position = new PluginNavigationPosition(0x02AA0100, 49.3, -65, 0, 0, false),
        };
        host.EventsValue.RaisePortalTransition(new PluginPortalTransition(
            0, 1, 0x02AA0100, true, true, true, false) { Kind = PluginPortalTransitionKind.Portal });

        Assert.True(objects.Identified.SequenceEqual(new uint[] { 44 }),
            $"Identified {string.Join(", ", objects.Identified)}; route {string.Join(" | ", destination.CurrentRoute!.Steps)}; reason {navigator.FailureReason}");
        Assert.Empty(host.Inner.PluginNavigation.GoToCalls);
        objects.Objects[2] = objects.Objects[2] with { PortalDestination = "Bandit Castle" };
        host.EventsValue.RaiseObjectChanged(new PluginObjectChange(44, PluginObjectChangeKind.IdentReceived));
        Assert.Equal((uint)44, Assert.Single(host.Inner.PluginNavigation.GoToCalls).ObjectId);
    }

    [Fact]
    public void UniqueSurfacePortalMustBeIdentifiedBeforeNavigationOrActivation()
    {
        var host = new NavigationHost();
        var objects = new TestWorldObjects();
        objects.Objects.Add(new PluginWorldObject(43, 0, "Surface Portal", PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(0x12340122, 95.5, 0, 0, 0, false),
        });
        host.Inner.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.Inner.PluginNavigation, Chat = host.Inner.PluginChat, Objects = objects,
        };
        host.Inner.PluginNavigation.SnapshotValue = new(true, false, 1,
            new PluginNavigationPosition(0x12340100, 95, 0, 0, 0, false), false, false);
        var db = new LocationDatabase();
        db.LoadLocationsXml("<locations><loc name='End' type='Town' NS='0' EW='98' /></locations>");
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();
        Assert.Equal(new uint[] { 43 }, objects.Identified);
        Assert.Empty(host.Inner.PluginNavigation.GoToCalls);
        Assert.Empty(objects.Activated);
        objects.Objects[0] = objects.Objects[0] with { PortalDestination = "Outdoor exit" };
        host.EventsValue.RaiseObjectChanged(new PluginObjectChange(43, PluginObjectChangeKind.IdentReceived));
        Assert.Equal((uint)43, Assert.Single(host.Inner.PluginNavigation.GoToCalls).ObjectId);
    }

    [Theory]
    [InlineData(false, false)]
    [InlineData(true, false)]
    [InlineData(false, true)]
    [InlineData(true, true)]
    public void BlackHillCrossingUsesNamedDistantExitInsteadOfSurfacePortal(bool exitAppearsLater, bool unnamedExit)
    {
        string atlasName = unnamedExit ? "Uncharted Passage" : "Desert March to Black Hill Portal";
        var host = new NavigationHost();
        var objects = new TestWorldObjects();
        objects.Objects.Add(new PluginWorldObject(42, 0, "Desert March", PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(0, 1, 0, 0, 0, true),
        });
        objects.Objects.Add(new PluginWorldObject(43, 0, "Surface Portal", PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(0x12340101, 95.01, 0, 0, 0, false),
            PortalDestination = unnamedExit ? "0.0N, 1.0E" : null,
        });
        var exit = new PluginWorldObject(44, 0, unnamedExit ? "Unmarked Portal" : "Black Hill", PluginObjectClass.Portal, 0, 0, 0)
        {
            PortalDestination = unnamedExit ? null : "Black Hill",
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(0x123401FF, 101.25, 0, 0, 0, false),
        };
        if (!exitAppearsLater)
            objects.Objects.Add(exit);
        host.Inner.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.Inner.PluginNavigation,
            Chat = host.Inner.PluginChat,
            Objects = objects,
        };
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(0, 0, 0, 0, 0, true), false, false);
        var db = new LocationDatabase();
        db.LoadLocationsXml($"""
            <locations>
              <loc name="{atlasName}" type="UndergroundPortal" NS="0" EW="1" exitNS="0" exitEW="95" />
              <loc name="End" type="Vendor" NS="0" EW="98" />
            </locations>
            """);
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();
        host.EventsValue.RaiseNavigationChanged(new PluginGoToReport(
            1, PluginGoToState.Arrived, 0, 0, 0, null) { Revision = 1 });
        Assert.Equal(new uint[] { 42 }, objects.Activated);
        host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
        {
            Position = new PluginNavigationPosition(0x12340100, 95, 0, 0, 0, false),
        };
        host.EventsValue.RaisePortalTransition(new PluginPortalTransition(
            0, 1, 0x12340100, true, true, true, false) { Kind = PluginPortalTransitionKind.Portal });
        if (exitAppearsLater)
        {
            Assert.Empty(host.Inner.PluginNavigation.GoToCalls);
            Assert.Contains(unnamedExit ? "End" : "Black Hill", navigator.FailureReason);
            Assert.False(navigator.PlanRoute());
            Assert.Empty(host.Inner.PluginNavigation.GoToCalls);
            objects.Objects.Add(exit);
            navigator.OnTick(0.6);
        }
        if (unnamedExit)
        {
            Assert.Contains((uint)44, objects.Identified);
            Assert.Empty(host.Inner.PluginNavigation.GoToCalls);
            int index = objects.Objects.FindIndex(p => p.ObjectId == 44);
            objects.Objects[index] = objects.Objects[index] with { PortalDestination = "Somewhere (0.0N, 95.0E)" };
            host.EventsValue.RaiseObjectChanged(new PluginObjectChange(44, PluginObjectChangeKind.IdentReceived));
        }
        Assert.Equal((uint)44, Assert.Single(host.Inner.PluginNavigation.GoToCalls).ObjectId);
        navigator.StopNavigation();
        navigator.ResumeNavigation();
        Assert.Equal(2, host.Inner.PluginNavigation.GoToCalls.Count);
        Assert.Equal((uint)44, host.Inner.PluginNavigation.GoToCalls[^1].ObjectId);
        navigator.StopNavigation();
        navigator.StartNavigation();
        Assert.Equal(3, host.Inner.PluginNavigation.GoToCalls.Count);
        Assert.Equal((uint)44, host.Inner.PluginNavigation.GoToCalls[^1].ObjectId);
        host.EventsValue.RaiseNavigationChanged(new PluginGoToReport(
            host.Inner.PluginNavigation.GoToReport.Sequence, PluginGoToState.Arrived, 44, 0, 0, null) { Revision = 2 });
        Assert.Equal(new uint[] { 42, 44 }, objects.Activated);
    }

    [Theory]
    [InlineData(false, false)]
    [InlineData(true, false)]
    [InlineData(false, true)]
    [InlineData(true, true)]
    public void LandscapeWalkThroughInteriorContinuesButPortalTransitDoesNot(bool portalTransit, bool nextIsDistantWalk)
    {
        var host = new NavigationHost();
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(0x35DB0010, 0, 0, 0, 0, true), false, false);
        var db = new LocationDatabase();
        db.LoadLocationsXml("""
            <locations>
              <loc name="First entrance" type="WildernessPortal" NS="0" EW="0.1" exitNS="0" exitEW="90" />
              <loc name="End" type="Town" NS="0" EW="100" />
            </locations>
            """);
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();
        if (nextIsDistantWalk)
            Assert.True(destination.RemoveRouteStep(1));
        if (portalTransit)
        {
            host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with { IsPortalSpace = true };
            navigator.OnTick(0.1);
        }
        host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
        {
            IsPortalSpace = false,
            Position = new PluginNavigationPosition(0x35DB0101, 0.1, 0, 0, 0, false),
        };
        navigator.OnTick(0.1);
        host.EventsValue.RaiseNavigationChanged(new PluginGoToReport(
            host.Inner.PluginNavigation.GoToReport.Sequence, PluginGoToState.Arrived, 0, 0, 0, null) { Revision = 1 });
        // The next leg is the entrance activation. A surface cave uses normal
        // portal searching; a portal transit still uses indoor resolution.
        Assert.Equal(portalTransit, navigator.WaitingForIndoorPortal);
        if (nextIsDistantWalk && !portalTransit)
        {
            Assert.True(navigator.IsNavigating);
            Assert.Equal(100, host.Inner.PluginNavigation.GoToPositionCalls[^1].Position.EastWest);
        }
    }

    [Theory]
    [InlineData(false, false, false)]
    [InlineData(true, false, false)]
    [InlineData(false, true, false)]
    [InlineData(true, true, false)]
    [InlineData(false, true, true)]
    [InlineData(false, false, true)]
    public void LivePortalIsUsedImmediatelyWhenNearOrOnceAfterCoordinateFailure(bool retryFails, bool portalNear, bool portalInSurfaceInterior)
    {
        var host = new NavigationHost();
        var objects = new TestWorldObjects();
        objects.Objects.Add(new PluginWorldObject(42, 0, "Portal to Town Network", PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(portalInSurfaceInterior ? 0x12340110u : 0x12340010u,
                2.05, 0, 0.1, 0, !portalInSurfaceInterior),
        });
        host.Inner.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.Inner.PluginNavigation,
            Chat = host.Inner.PluginChat,
            Objects = objects,
        };
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(0, portalNear ? 1.95 : 1, 0, 0, 0, true), false, false);
        var db = new LocationDatabase();
        db.LoadLocationsXml("""
            <locations>
              <loc name="Town Network Portal(Bluespire)" type="WildernessPortal" NS="0" EW="2" exitNS="0" exitEW="95" />
              <loc name="End" type="Town" NS="0" EW="96" />
            </locations>
            """);
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();
        if (portalInSurfaceInterior)
        {
            host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
            {
                Position = new PluginNavigationPosition(0x12340120, 1.95, 0, 0, 0, false),
            };
            navigator.OnTick(0.1);
        }
        if (portalNear)
            Assert.Empty(host.Inner.PluginNavigation.GoToPositionCalls);
        else
            host.EventsValue.RaiseNavigationChanged(
                new PluginGoToReport(1, PluginGoToState.NoRoute, 0, 36, 0, "No reachable spot can see the goal") { Revision = 1 });
        Assert.True(navigator.IsNavigating);
        Assert.Equal((uint)42, Assert.Single(host.Inner.PluginNavigation.GoToCalls).ObjectId);
        host.EventsValue.RaiseNavigationChanged(new PluginGoToReport(
            host.Inner.PluginNavigation.GoToReport.Sequence,
            retryFails ? PluginGoToState.NoRoute : PluginGoToState.Arrived,
            42, 0, 0, retryFails ? "Live portal is also unreachable" : null) { Revision = 2 });
        Assert.Single(host.Inner.PluginNavigation.GoToCalls);
        if (retryFails)
        {
            Assert.False(navigator.IsNavigating);
            Assert.Empty(objects.Activated);
            Assert.Equal("Live portal is also unreachable", navigator.FailureReason);
        }
        else
        {
            Assert.Equal(new uint[] { 42 }, objects.Activated);
            Assert.True(navigator.WaitingForInteraction);
            Assert.Equal(RouteStepKind.Portal, destination.CurrentRoute!.Steps[0].Kind);
        }
    }

    [Theory]
    [InlineData("Sawato", "Unmarked Portal", "Sawato")]
    [InlineData("Sawato", "Sawato Portal", "28.7S, 59.3E")]
    [InlineData("Sawato", "Portal to Sawato", "Sawato (28.7S, 59.3E)")]
    public void IndoorPortalMatchesNameOrDestinationLabel(string targetName, string objectName, string destinationLabel)
    {
        var host = new NavigationHost();
        var objects = new TestWorldObjects();
        objects.Objects.Add(new PluginWorldObject(43, 0, objectName, PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(0x12340122, 40, 40, 0, 0, false),
            PortalDestination = destinationLabel,
        });
        host.Inner.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.Inner.PluginNavigation,
            Chat = host.Inner.PluginChat,
            Objects = objects,
        };
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1,
            new PluginNavigationPosition(0x12340100, 39, 40, 0, 0, false),
            false, false
        );
        var db = new LocationDatabase();
        db.LoadLocationsXml($"<locations><loc name='{targetName}' type='Town' NS='80' EW='80' /></locations>");
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination(targetName));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.StartNavigation();

        Assert.Equal((uint)43, Assert.Single(host.Inner.PluginNavigation.GoToCalls).ObjectId);
    }

    [Theory]
    [InlineData(0.5)]
    [InlineData(6.25)]
    public void StartingInsideDungeonCanUseUniqueSurfacePortalForOutdoorDestination(double exitOffset)
    {
        var host = new NavigationHost();
        var objects = new TestWorldObjects();
        objects.Objects.Add(new PluginWorldObject(43, 0, "Surface Portal", PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            PortalDestination = "Outdoor exit",
            Position = new PluginNavigationPosition(0x12340122, 95 + exitOffset, 0, 0, 0, false),
        });
        host.Inner.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.Inner.PluginNavigation,
            Chat = host.Inner.PluginChat,
            Objects = objects,
        };
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1,
            new PluginNavigationPosition(0x12340100, 95, 0, 0, 0, false),
            false, false
        );
        var db = new LocationDatabase();
        db.LoadLocationsXml("<locations><loc name='End' type='Town' NS='0' EW='98' /></locations>");
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();

        Assert.Equal((uint)43, Assert.Single(host.Inner.PluginNavigation.GoToCalls).ObjectId);
        host.EventsValue.RaiseNavigationChanged(
            new PluginGoToReport(1, PluginGoToState.Arrived, 43, 0, 0, null) { Revision = 1 }
        );
        Assert.Equal(new uint[] { 43 }, objects.Activated);
        host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
        {
            Position = new PluginNavigationPosition(0, 96, 0, 0, 0, true),
        };
        navigator.OnTick(0.1);
        Assert.True(navigator.IsNavigating);
        Assert.Equal(98, Assert.Single(host.Inner.PluginNavigation.GoToPositionCalls).Position.EastWest);
    }

    [Fact]
    public void MultipleSurfacePortalsDoNotSelectAnExitAutomatically()
    {
        var host = new NavigationHost();
        var objects = new TestWorldObjects();
        foreach (uint id in new uint[] { 43, 44 })
            objects.Objects.Add(new PluginWorldObject(id, 0, "Surface Portal", PluginObjectClass.Portal, 0, 0, 0)
            {
                Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
                HasPosition = true,
                Position = new PluginNavigationPosition(0x12340122, 95 + id - 43, 0, 0, 0, false),
            });
        host.Inner.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.Inner.PluginNavigation,
            Chat = host.Inner.PluginChat,
            Objects = objects,
        };
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1,
            new PluginNavigationPosition(0x12340100, 95, 0, 0, 0, false),
            false, false
        );
        var db = new LocationDatabase();
        db.LoadLocationsXml("<locations><loc name='End' type='Town' NS='0' EW='98' /></locations>");
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.StartNavigation();

        Assert.Empty(host.Inner.PluginNavigation.GoToCalls);
        Assert.Empty(objects.Activated);
    }

    [Theory]
    [InlineData(true)]
    [InlineData(false)]
    public void AtlasRouteUsesSurfacePortalDestinationThenReplansOutdoors(bool destinationKnown)
    {
        var host = new NavigationHost();
        var objects = new TestWorldObjects();
        objects.Objects.Add(new PluginWorldObject(42, 0, "Black Hill Portal", PluginObjectClass.Portal, 0, 0, 0)
        {
            PortalDestination = "Dungeon entrance",
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(0, 1, 0, 0, 0, true),
        });
        objects.Objects.Add(new PluginWorldObject(43, 0, "Surface Portal", PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(0x12340122, 95.5, 0, 0, 0, false),
            PortalDestination = destinationKnown ? "Direlands South Landbridge" : null,
            HasAppraisalData = destinationKnown,
        });
        objects.Objects.Add(new PluginWorldObject(44, 0, "Surface Portal", PluginObjectClass.Portal, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(0x12340123, 95.7, 0, 0, 0, false),
            PortalDestination = "Somewhere Else",
            HasAppraisalData = true,
        });
        host.Inner.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.Inner.PluginNavigation,
            Chat = host.Inner.PluginChat,
            Objects = objects,
        };
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(0, 0, 0, 0, 0, true), false, false
        );
        var db = new LocationDatabase();
        db.LoadLocationsXml(
            """
            <locations>
              <loc name="Start" type="Town" NS="0" EW="0" />
              <loc name="Black Hill Portal" type="UndergroundPortal" NS="0" EW="1" exitNS="0" exitEW="95" />
              <loc name="Portal to Direlands South Landbridge" type="WildernessPortal" NS="0" EW="98" exitNS="0" exitEW="200" />
              <loc name="End" type="Town" NS="0" EW="107" />
            </locations>
            """
        );
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();
        host.EventsValue.RaiseNavigationChanged(
            new PluginGoToReport(1, PluginGoToState.Arrived, 0, 0, 0, null) { Revision = 1 }
        );
        Assert.Equal(new uint[] { 42 }, objects.Activated);

        host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
        {
            Position = new PluginNavigationPosition(0x12340100, 95, 0, 0, 0, false),
        };
        host.EventsValue.RaisePortalTransition(
            new PluginPortalTransition(0, 1, 0x12340100, true, true, true, false)
            { Kind = PluginPortalTransitionKind.Portal }
        );
        if (!destinationKnown)
        {
            Assert.Equal(new uint[] { 43 }, objects.Identified);
            Assert.Empty(host.Inner.PluginNavigation.GoToCalls);
            objects.Objects[1] = objects.Objects[1] with
            {
                PortalDestination = "Direlands South Landbridge",
                HasAppraisalData = true,
            };
            host.EventsValue.RaiseObjectChanged(
                new PluginObjectChange(43, PluginObjectChangeKind.IdentReceived)
            );
        }
        Assert.Equal((uint)43, Assert.Single(host.Inner.PluginNavigation.GoToCalls).ObjectId);
        Assert.Equal(RouteStepKind.Travel, destination.CurrentRoute!.Steps[0].Kind);
        host.EventsValue.RaiseNavigationChanged(
            new PluginGoToReport(2, PluginGoToState.Arrived, 43, 0, 0, null) { Revision = 2 }
        );
        Assert.Equal(new uint[] { 42, 43 }, objects.Activated);
        Assert.True(navigator.WaitingForInteraction);
        Assert.Equal(RouteStepKind.Travel, destination.CurrentRoute.Steps[0].Kind);

        host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
        {
            Position = new PluginNavigationPosition(0, 98, 0, 0, 0, true),
        };
        navigator.OnTick(0.1);
        Assert.False(navigator.WaitingForInteraction);
        Assert.True(navigator.IsNavigating);
        Assert.Equal(107, host.Inner.PluginNavigation.GoToPositionCalls[^1].Position.EastWest);
    }

    [Theory]
    [InlineData(true, false, false, false, false, false, false)]
    [InlineData(false, false, false, false, false, false, false)]
    [InlineData(false, true, false, false, false, false, false)]
    [InlineData(false, true, false, false, true, false, false)]
    [InlineData(true, false, true, false, false, false, false)]
    [InlineData(false, false, false, true, false, false, false)]
    [InlineData(false, false, false, false, false, true, false)]
    [InlineData(false, false, false, false, false, false, true)]
    public void OutdoorRouteContinuesThroughTownNetworkIndoorPortal(
        bool receivesTransitionEvent,
        bool exitsBeforeIndoorWalkArrives,
        bool portalEventPrecedesIndoorPosition,
        bool portalInNextLandblock,
        bool portalSpaceSeenOnTick,
        bool resumeDuringIndoorWalk,
        bool resumeBeforeEntryStepCompletes
    )
    {
        var host = new NavigationHost();
        var objects = new TestWorldObjects();
        objects.Objects.Add(
            new PluginWorldObject(
                42,
                0,
                "Portal to Town Network",
                PluginObjectClass.Portal,
                0,
                0,
                0
            )
            {
                Capabilities =
                    PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
                HasPosition = true,
                Position = new PluginNavigationPosition(0, 2, 0, 0, 0, true),
            }
        );
        var sawatoPortal = new PluginWorldObject(
            43,
            0,
            "Sawato Portal",
            PluginObjectClass.Portal,
            0,
            0,
            0
        )
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            PortalDestination = "Sawato (80N, 80E)",
            HasPosition = true,
            Position = new PluginNavigationPosition(
                portalInNextLandblock ? 0x12350122u : 0x12340122u,
                40.02,
                40,
                0,
                0,
                false
            ),
        };
        host.Inner.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.Inner.PluginNavigation,
            Chat = host.Inner.PluginChat,
            Objects = objects,
        };
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true,
            false,
            1,
            new PluginNavigationPosition(0, 1, 0, 0, 0, true),
            false,
            false
        );
        var db = new LocationDatabase();
        db.LoadLocationsXml(
            """
            <locations>
              <loc name="Start" type="Town" NS="0" EW="1" />
              <loc name="Town Network Portal(Shoushi)" type="TownPortal"
                NS="0" EW="2" exitNS="40" exitEW="40" />
              <loc name="Town Network (E R 5) to Sawato" type="UndergroundPortal"
                NS="40" EW="40.02" exitNS="80" exitEW="80" />
              <loc name="Sawato" type="Town" NS="80" EW="80.57" />
            </locations>
            """
        );
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("Sawato"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();

        Assert.Equal(
            2,
            Assert.Single(host.Inner.PluginNavigation.GoToPositionCalls).Position.EastWest
        );
        host.EventsValue.RaiseNavigationChanged(
            new PluginGoToReport(1, PluginGoToState.Arrived, 0, 0, 0, null) { Revision = 1 }
        );
        Assert.Equal(new uint[] { 42 }, objects.Activated);
        Assert.Equal(RouteStepKind.Portal, destination.CurrentRoute!.Steps[0].Kind);

        if (portalEventPrecedesIndoorPosition)
        {
            host.EventsValue.RaisePortalTransition(
                new PluginPortalTransition(0, 1, 0x12340100, true, true, true, false)
                {
                    Kind = PluginPortalTransitionKind.Portal,
                }
            );
            Assert.Equal(RouteStepKind.Travel, destination.CurrentRoute.Steps[0].Kind);
            host.Inner.PluginNavigation.SnapshotValue = host.Inner
                .PluginNavigation
                .SnapshotValue with
            {
                Position = new PluginNavigationPosition(0, 80, 80, 0, 0, true),
            };
            navigator.OnTick(0.1);
            Assert.Equal(
                80.57,
                host.Inner.PluginNavigation.GoToPositionCalls[^1].Position.EastWest,
                3
            );
            Assert.True(navigator.IsNavigating);
            return;
        }

        if (!receivesTransitionEvent)
        {
            host.Inner.PluginNavigation.SnapshotValue = host.Inner
                .PluginNavigation
                .SnapshotValue with
            {
                IsPortalSpace = true,
            };
            navigator.OnTick(0.1);
            Assert.Equal(RouteStepKind.Portal, destination.CurrentRoute.Steps[0].Kind);
        }

        host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
        {
            IsPortalSpace = false,
            Position = new PluginNavigationPosition(0x12340100, resumeBeforeEntryStepCompletes ? 42 : 40, 40, 0, 0, false),
        };
        if (resumeBeforeEntryStepCompletes)
        {
            navigator.StopNavigation();
            Assert.Equal(RouteStepKind.Portal, destination.CurrentRoute.Steps[0].Kind);
            navigator.ResumeNavigation();
        }
        else if (receivesTransitionEvent)
            host.EventsValue.RaisePortalTransition(
                new PluginPortalTransition(0, 1, 0x12340100, true, true, true, false)
                {
                    Kind = PluginPortalTransitionKind.Portal,
                }
            );
        else
            navigator.OnTick(0.1);

        Assert.Equal(RouteStepKind.Travel, destination.CurrentRoute.Steps[0].Kind);
        Assert.True(navigator.WaitingForIndoorPortal);
        Assert.Empty(host.Inner.PluginNavigation.GoToCalls);
        objects.Objects.Add(sawatoPortal);
        navigator.OnTick(0.6);
        Assert.Equal((uint)43, Assert.Single(host.Inner.PluginNavigation.GoToCalls).ObjectId);
        if (resumeDuringIndoorWalk)
        {
            navigator.StopNavigation();
            Assert.True(navigator.CanResumeNavigation);
            navigator.ResumeNavigation();
            Assert.Equal((uint)43, host.Inner.PluginNavigation.GoToCalls[^1].ObjectId);
            Assert.Equal(2, host.Inner.PluginNavigation.GoToCalls.Count);
            Assert.True(navigator.IsNavigating);
        }
        if (portalInNextLandblock)
        {
            host.Inner.PluginNavigation.SnapshotValue = host.Inner
                .PluginNavigation
                .SnapshotValue with
            {
                Position = new PluginNavigationPosition(0x12360100, 40.01, 40, 0, 0, false),
            };
            navigator.OnTick(0.1);
            Assert.True(navigator.IsNavigating);
            Assert.Equal(0, host.Inner.PluginNavigation.StopGoToCount);
        }
        if (exitsBeforeIndoorWalkArrives)
        {
            host.Inner.PluginNavigation.SnapshotValue = host.Inner
                .PluginNavigation
                .SnapshotValue with
            {
                IsPortalSpace = true,
            };
            if (portalSpaceSeenOnTick)
                navigator.OnTick(0.1);
            else
                host.EventsValue.RaiseNavigationChanged(
                    new PluginGoToReport(2, PluginGoToState.Lost, 43, 0, 0, "Portal space")
                    {
                        Revision = 2,
                    }
                );
            Assert.Equal("Waiting for portal transition.", navigator.FailureReason);
            Assert.Equal(RouteStepKind.Travel, destination.CurrentRoute.Steps[0].Kind);
            host.Inner.PluginNavigation.SnapshotValue = host.Inner
                .PluginNavigation
                .SnapshotValue with
            {
                IsPortalSpace = false,
                Position = new PluginNavigationPosition(0, 80, 80, 0, 0, true),
            };
            navigator.OnTick(0.1);
            Assert.Equal(
                80.57,
                host.Inner.PluginNavigation.GoToPositionCalls[1].Position.EastWest,
                3
            );
            Assert.True(navigator.IsNavigating);
            Assert.Equal(RouteStepKind.Travel, destination.CurrentRoute.Steps[0].Kind);
            return;
        }
        host.EventsValue.RaiseNavigationChanged(
            new PluginGoToReport(
                host.Inner.PluginNavigation.GoToReport.Sequence,
                PluginGoToState.Arrived,
                43,
                0,
                0,
                null
            )
            {
                Revision = 2,
            }
        );
        Assert.Equal(new uint[] { 42, 43 }, objects.Activated);
        Assert.Equal(RouteStepKind.Portal, destination.CurrentRoute.Steps[0].Kind);

        host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
        {
            Position = new PluginNavigationPosition(0, 80, 80, 0, 0, true),
        };
        if (receivesTransitionEvent)
            host.EventsValue.RaisePortalTransition(
                new PluginPortalTransition(0, 2, 0, true, true, true, false)
                {
                    Kind = PluginPortalTransitionKind.Portal,
                }
            );
        else
            navigator.OnTick(0.1);

        Assert.Equal(80.57, host.Inner.PluginNavigation.GoToPositionCalls[1].Position.EastWest, 3);
        Assert.Equal(RouteStepKind.Travel, destination.CurrentRoute.Steps[0].Kind);

        navigator.OnTick(16);
        Assert.True(navigator.IsNavigating);
        Assert.Equal(2, host.Inner.PluginNavigation.GoToPositionCalls.Count);
        host.EventsValue.RaiseNavigationChanged(
            new PluginGoToReport(
                host.Inner.PluginNavigation.GoToReport.Sequence,
                PluginGoToState.ArrivedWithoutSight,
                0,
                400,
                0,
                "No accessible route"
            )
            {
                Revision = 3,
            }
        );
        Assert.False(navigator.HasArrived);
        Assert.False(navigator.IsNavigating);
        Assert.Equal(RouteStepKind.Travel, destination.CurrentRoute.Steps[0].Kind);
        Assert.Contains("400 m remaining", navigator.FailureReason);
    }

    [Theory]
    [InlineData(false, false)]
    [InlineData(true, false)]
    [InlineData(false, true)]
    public void PortalRouteWaitsForTransitionThenContinuesWalking(
        bool arrivesIndoors,
        bool manualResumeWithAutoNavigateOff
    )
    {
        var host = new NavigationHost();
        var objects = new TestWorldObjects();
        objects.Objects.Add(
            new PluginWorldObject(42, 0, "Entrance to Somewhere", default, 0, 0, 0)
            {
                Capabilities =
                    PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
                HasPosition = true,
                Position = new PluginNavigationPosition(0, 2, 0, 0, 0, true),
            }
        );
        host.Inner.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.Inner.PluginNavigation,
            Chat = host.Inner.PluginChat,
            Objects = objects,
        };
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true,
            false,
            1,
            new PluginNavigationPosition(0, 1, 0, 0, 0, true),
            false,
            false
        );

        var db = new LocationDatabase();
        db.LoadLocationsXml(
            """
            <atlas>
              <location><id>1</id><name>Start</name><type>Town</type><latitude>0</latitude><longitude>1</longitude><retired>N</retired></location>
              <location><id>2</id><name>Far Portal</name><type>Wilderness Portal</type><latitude>0</latitude><longitude>2</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>95</arrival_longitude><retired>N</retired></location>
              <location><id>3</id><name>End</name><type>Town</type><latitude>0</latitude><longitude>96</longitude><retired>N</retired></location>
            </atlas>
            """
        );
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();

        Assert.Equal(2, host.Inner.PluginNavigation.GoToPositionCalls[0].Position.EastWest);
        host.EventsValue.RaiseNavigationChanged(
            new PluginGoToReport(1, PluginGoToState.Arrived, 0, 0, 0, null) { Revision = 1 }
        );

        Assert.Equal(new uint[] { 42 }, objects.Activated);
        Assert.True(navigator.WaitingForInteraction);
        Assert.Equal(RouteStepKind.Portal, destination.CurrentRoute!.Steps[0].Kind);
        Assert.Single(host.Inner.PluginNavigation.GoToPositionCalls);

        host.EventsValue.RaiseActivationCompleted(
            new PluginActivationCompletion(1, 42, PluginActivationOutcome.Completed, 0)
        );
        Assert.True(navigator.WaitingForInteraction);
        Assert.Equal(RouteStepKind.Portal, destination.CurrentRoute.Steps[0].Kind);
        Assert.Single(host.Inner.PluginNavigation.GoToPositionCalls);

        host.EventsValue.RaisePortalTransition(
            new PluginPortalTransition(1, 1, 0, true, true, true, false)
            {
                Kind = PluginPortalTransitionKind.Login,
            }
        );
        Assert.True(navigator.WaitingForInteraction);
        if (manualResumeWithAutoNavigateOff)
        {
            settings.AutoNavigate = false;
            host.Inner.PluginNavigation.SnapshotValue = host.Inner
                .PluginNavigation
                .SnapshotValue with
            {
                Position = new PluginNavigationPosition(0, 95, 0, 0, 0, true),
            };
            navigator.ResumeNavigation();
            Assert.False(navigator.WaitingForInteraction);
            Assert.False(navigator.IsNavigating);
            Assert.Equal(RouteStepKind.Travel, destination.CurrentRoute.Steps[0].Kind);
            Assert.Single(host.Inner.PluginNavigation.GoToPositionCalls);
            return;
        }
        if (arrivesIndoors)
            host.Inner.PluginNavigation.SnapshotValue = host.Inner
                .PluginNavigation
                .SnapshotValue with
            {
                Position = new PluginNavigationPosition(0x12340100, 95, 0, 0, 0, false),
            };
        host.EventsValue.RaisePortalTransition(
            new PluginPortalTransition(0, 2, 0, true, true, true, false)
            {
                Kind = PluginPortalTransitionKind.Portal,
            }
        );

        Assert.False(navigator.WaitingForInteraction);
        if (arrivesIndoors)
        {
            Assert.Single(host.Inner.PluginNavigation.GoToPositionCalls);
            host.Inner.PluginNavigation.SnapshotValue = host.Inner
                .PluginNavigation
                .SnapshotValue with
            {
                Position = new PluginNavigationPosition(0, 95, 0, 0, 0, true),
            };
            navigator.OnTick(0.1);
        }
        Assert.Equal(96, host.Inner.PluginNavigation.GoToPositionCalls[1].Position.EastWest);
        Assert.Equal(RouteStepKind.Travel, destination.CurrentRoute.Steps[0].Kind);
    }

    private sealed class TestWorldObjects : IWorldObjectAutomation
    {
        public List<PluginWorldObject> Objects { get; } = [];
        public List<uint> Activated { get; } = [];
        public List<uint> Identified { get; } = [];

        public IReadOnlyList<PluginWorldObject> CaptureObjects() => Objects;

        public PluginItemCommandResult Identify(uint objectId)
        {
            Identified.Add(objectId);
            return new PluginItemCommandResult(PluginItemCommandStatus.Started);
        }

        public PluginItemCommandResult Activate(uint objectId)
        {
            Activated.Add(objectId);
            return new PluginItemCommandResult(PluginItemCommandStatus.Started);
        }
    }

    private sealed class NavigationEvents : IEvents
    {
        public event Action<WorldEntitySnapshot>? EntitySpawned
        {
            add { }
            remove { }
        }
        public event Action<double>? Tick
        {
            add { }
            remove { }
        }
        public event Action<PluginGoToReport>? NavigationChanged;
        public event Action<PluginActivationCompletion>? ActivationCompleted;
        public event Action<PluginPortalTransition>? PortalTransition;
        public event Action<PluginObjectChange>? ObjectChanged;

        public void RaiseNavigationChanged(PluginGoToReport report) =>
            NavigationChanged?.Invoke(report);

        public void RaiseActivationCompleted(PluginActivationCompletion completion) =>
            ActivationCompleted?.Invoke(completion);

        public void RaisePortalTransition(PluginPortalTransition transition) =>
            PortalTransition?.Invoke(transition);

        public void RaiseObjectChanged(PluginObjectChange change) => ObjectChanged?.Invoke(change);
    }

    private sealed class NavigationHost : IPluginHost
    {
        public FakePluginHost Inner { get; } = new();
        public NavigationEvents EventsValue { get; } = new();
        public bool HasUi => Inner.HasUi;
        public IPluginLogger Log => Inner.Log;
        public IGameState State => Inner.State;
        public IEvents Events => EventsValue;
        public ISelectionService Selection => Inner.Selection;
        public IUiRegistry Ui => Inner.Ui;
        public IPluginCommandRegistry Commands => Inner.Commands;
        public IPluginStorage Storage => Inner.Storage;
        public IAutomationSurface Automation => Inner.Automation;
        public IPluginStorage VtankProfiles => Inner.VtankProfiles;
        public IPluginClipboard Clipboard => Inner.Clipboard;
        public IHostWindow Window => Inner.Window;
        public IReadOnlyDictionary<string, string> SessionSettings => Inner.SessionSettings;
    }
}
