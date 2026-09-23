using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public class RouteGraphTests
{
    // ── Helpers ─────────────────────────────────────────────────────

    private static (LocationDatabase, RouteGraph) CreateGraph(params string[] locCsv)
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(locCsv);
        var graph = new RouteGraph();
        graph.Build(db, maxWalkDistance: 10.0);
        return (db, graph);
    }

    private static (LocationDatabase, RouteGraph) CreateGraphWithPortalAndStarts()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(
            new[] { "TownA;0;0", "TownB;10;0", "TownC;10;10", "TownD;0;10", "PortalDest;5;5" }
        );
        db.LoadPortalDevicesCsv(new[] { "PortalDest;Magic Portal;Dereth" });
        db.LoadRouteStartsCsv(new[] { "TownB;TownA;Walk" });
        var graph = new RouteGraph();
        graph.Build(db, maxWalkDistance: 10.0);
        return (db, graph);
    }

    // ── Build tests ─────────────────────────────────────────────────

    [Fact]
    public void Build_CreatesNodesForEligibleLocations()
    {
        var (_, graph) = CreateGraph("Alpha;0;0", "Beta;10;5", "Gamma;20;10");

        Assert.Equal(3, graph.NodeCount);
        Assert.True(graph.GetNodeIndex("Alpha") >= 0);
        Assert.True(graph.GetNodeIndex("Beta") >= 0);
        Assert.True(graph.GetNodeIndex("Gamma") >= 0);
    }

    [Fact]
    public void Build_ExcludesRetiredLocations()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(new[] { "Active;0;0", "Retired;10;10" });
        // Mark second as retired
        var retired = db.FindLocation("Retired");
        if (retired != null)
        {
            retired.IsRetired = true;
            retired.UseInRouteFinding = false;
        }

        var graph = new RouteGraph();
        graph.Build(db);

        Assert.Equal(1, graph.NodeCount);
        Assert.True(graph.GetNodeIndex("Active") >= 0);
        Assert.True(graph.GetNodeIndex("Retired") < 0);
    }

    [Fact]
    public void Build_ExcludesLocationsWithoutCoordinates()
    {
        var db = new LocationDatabase();
        db.LoadLocationsXml("<Locations><Location name='NoCoords'/></Locations>");
        db.LoadLocationsCsv(new[] { "HasCoords;10;20" });

        var graph = new RouteGraph();
        graph.Build(db);

        Assert.Equal(1, graph.NodeCount);
        Assert.True(graph.GetNodeIndex("NoCoords") < 0);
    }

    [Fact]
    public void Build_CreatesWalkEdgesBetweenNearbyLocations()
    {
        var (_, graph) = CreateGraph(
            "Alpha;0;0",
            "Beta;5;0", // 5 mu from Alpha
            "Gamma;15;0"
        ); // 15 mu from Alpha (beyond maxWalkDistance)

        var alphaIdx = graph.GetNodeIndex("Alpha");
        var betaIdx = graph.GetNodeIndex("Beta");
        var gammaIdx = graph.GetNodeIndex("Gamma");

        var alphaEdges = graph.GetEdges(alphaIdx);
        var betaEdges = graph.GetEdges(betaIdx);

        // Alpha should connect to Beta (5 <= 10)
        Assert.Contains(alphaEdges, e => e.ToIndex == betaIdx && e.Kind == RouteEdgeKind.Walk);
        // Beta should connect to Alpha
        Assert.Contains(betaEdges, e => e.ToIndex == alphaIdx && e.Kind == RouteEdgeKind.Walk);

        // Alpha should NOT connect to Gamma (15 > 10)
        Assert.DoesNotContain(alphaEdges, e => e.ToIndex == gammaIdx);
    }

    [Fact]
    public void Build_IncludesExplicitPortalEdges()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(new[] { "Home;0;0", "FarTown;50;0" });
        db.LoadPortalDevicesCsv(new[] { "FarTown;Far Portal;Dereth;Home;FarTown" });

        var graph = new RouteGraph();
        graph.Build(db, maxWalkDistance: 10.0);

        var path = graph.FindShortestPath("Home", "FarTown");

        Assert.NotNull(path);
        Assert.Single(path!);
        Assert.Equal(RouteEdgeKind.Portal, path[0].Kind);
        Assert.Equal("Far Portal", path[0].Via);
    }

    [Fact]
    public void Build_IncludesRouteStartEdges()
    {
        var (_, graph) = CreateGraphWithPortalAndStarts();

        var townAIdx = graph.GetNodeIndex("TownA");
        var townBIdx = graph.GetNodeIndex("TownB");

        // Route start: TownB;TownA;Walk → edge from TownA to TownB via "Walk"
        var edges = graph.GetEdges(townAIdx);
        Assert.Contains(edges, e => e.ToIndex == townBIdx && e.Kind == RouteEdgeKind.Walk);
    }

    [Fact]
    public void Build_WalkEdgesAreUndirected()
    {
        var (_, graph) = CreateGraph("A;0;0", "B;5;0");

        var aEdges = graph.GetEdges(graph.GetNodeIndex("A"));
        var bEdges = graph.GetEdges(graph.GetNodeIndex("B"));

        Assert.Contains(aEdges, e => e.ToIndex == graph.GetNodeIndex("B"));
        Assert.Contains(bEdges, e => e.ToIndex == graph.GetNodeIndex("A"));
    }

    [Fact]
    public void Build_DeduplicatesByName()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(
            new[]
            {
                "Town;0;0",
                "Town;1;1", // same name, different coords
                "Town;2;2", // same name again
            }
        );

        var graph = new RouteGraph();
        graph.Build(db);

        // Only one node for "Town" regardless of duplicates
        Assert.Equal(1, graph.NodeCount);
        var idx = graph.GetNodeIndex("Town");
        Assert.True(idx >= 0);
        // First occurrence wins (ordered by name then id)
        Assert.Equal(0, graph.GetLocation(idx).NS);
    }

    // ── Shortest Path tests ─────────────────────────────────────────

    [Fact]
    public void FindShortestPath_DirectWalk_ReturnsSingleEdge()
    {
        var (_, graph) = CreateGraph("A;0;0", "B;5;0");

        var path = graph.FindShortestPath("A", "B");

        Assert.NotNull(path);
        Assert.Single(path);
        Assert.Equal(RouteEdgeKind.Walk, path![0].Kind);
    }

    [Fact]
    public void FindShortestPath_MultiHop_ReturnsFullPath()
    {
        // A (0,0) → B (5,0) → C (11,0) — chain of walk edges.
        // A→C = 11 mu > maxWalkDistance (10), so A must route via B.
        var (_, graph) = CreateGraph("A;0;0", "B;5;0", "C;11;0");

        var path = graph.FindShortestPath("A", "C");

        Assert.NotNull(path);
        Assert.Equal(2, path!.Count);
        Assert.Equal(RouteEdgeKind.Walk, path[0].Kind);
        Assert.Equal(RouteEdgeKind.Walk, path[1].Kind);
        Assert.Equal(graph.GetNodeIndex("B"), path[0].ToIndex);
        Assert.Equal(graph.GetNodeIndex("C"), path[1].ToIndex);
    }

    [Fact]
    public void FindShortestPath_SameStartAndEnd_ReturnsEmptyPath()
    {
        var (_, graph) = CreateGraph("A;0;0", "B;5;0");

        var path = graph.FindShortestPath("A", "A");

        Assert.NotNull(path);
        Assert.Empty(path);
    }

    [Fact]
    public void FindShortestPath_UnknownNode_ReturnsNull()
    {
        var (_, graph) = CreateGraph("A;0;0");

        Assert.Null(graph.FindShortestPath("A", "Unknown"));
        Assert.Null(graph.FindShortestPath("Unknown", "A"));
    }

    [Fact]
    public void FindShortestPath_Unreachable_ReturnsNull()
    {
        // A (0,0), B (50,0) — too far for walk edge, no other connections
        var (_, graph) = CreateGraph("A;0;0", "B;50;0");

        var path = graph.FindShortestPath("A", "B");

        Assert.Null(path);
    }

    [Fact]
    public void FindShortestPath_UsesRouteStartEdges()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(new[] { "Home;0;0", "Town;50;0" });
        db.LoadRouteStartsCsv(new[] { "Town;Home;Portal Recall" });
        var graph = new RouteGraph();
        graph.Build(db, maxWalkDistance: 10.0);

        // Walk is impossible (50 > 10), but route start provides an edge
        var path = graph.FindShortestPath("Home", "Town");

        Assert.NotNull(path);
        Assert.Single(path);
        Assert.Equal(RouteEdgeKind.Recall, path![0].Kind);
        Assert.Equal("Portal Recall", path[0].Via);
    }

    [Fact]
    public void FindShortestPath_PrefersShorterRoute()
    {
        // A → B is walkable (3 mu), A → C → B is also walkable but longer
        var (_, graph) = CreateGraph("A;0;0", "B;3;0", "C;1;5");

        var path = graph.FindShortestPath("A", "B");

        Assert.NotNull(path);
        // Direct A→B should be shorter than A→C→B
        Assert.Single(path);
        Assert.Equal(graph.GetNodeIndex("B"), path![0].ToIndex);
    }

    [Fact]
    public void FindShortestPath_CustomCostSelectorUsesDeterministicWeightedPath()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(new[] { "A;0;0", "B;2;0", "C;1;1", "D;3;0" });
        var graph = new RouteGraph();
        graph.Build(db, maxWalkDistance: 10.0);

        var path = graph.FindShortestPath(
            graph.GetNodeIndex("A"),
            graph.GetNodeIndex("D"),
            edge =>
                edge.Kind == RouteEdgeKind.Walk
                && edge.Via.Contains("Walk", StringComparison.OrdinalIgnoreCase)
                    ? edge.Cost
                    : edge.Cost * 100
        );

        Assert.NotNull(path);
        Assert.Equal(graph.GetNodeIndex("D"), path![^1].ToIndex);
    }

    [Fact]
    public void FindShortestPath_MultiHopWithRouteStarts()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(new[] { "Start;0;0", "Middle;5;0", "Far;50;0" });
        db.LoadRouteStartsCsv(new[] { "Far;Middle;Recall" });
        var graph = new RouteGraph();
        graph.Build(db, maxWalkDistance: 10.0);

        // Start → Middle (walk, 5mu) → Far (recall via route start)
        var path = graph.FindShortestPath("Start", "Far");

        Assert.NotNull(path);
        Assert.Equal(2, path!.Count);
        Assert.Equal(RouteEdgeKind.Walk, path[0].Kind);
        Assert.Equal(RouteEdgeKind.Recall, path[1].Kind);
    }

    // ── ToRoute tests ────────────────────────────────────────────────

    [Fact]
    public void ToRoute_ConvertsWalkPath()
    {
        var (_, graph) = CreateGraph("A;0;0", "B;5;0");

        var path = graph.FindShortestPath("A", "B");
        var route = graph.ToRoute(path!, "B");

        Assert.Equal("B", route.Destination);
        Assert.Equal(1, route.StepCount);
        Assert.Equal(RouteStepKind.Travel, route.Steps[0].Kind);
        Assert.Equal("Walk", route.Steps[0].Via);
    }

    [Fact]
    public void ToRoute_ConvertsMultiHopPath()
    {
        // A→B = 5mu, B→C = 6mu, A→C = 11mu > maxWalkDistance = 10
        var (_, graph) = CreateGraph("A;0;0", "B;5;0", "C;11;0");

        var path = graph.FindShortestPath("A", "C");
        var route = graph.ToRoute(path!, "C");

        Assert.Equal(2, route.StepCount);
        Assert.Equal(route.Steps[0].To, route.Steps[1].From);
    }

    [Fact]
    public void ToRoute_ConvertsRouteStartEdges()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(new[] { "Home;0;0", "Town;50;0" });
        db.LoadRouteStartsCsv(new[] { "Town;Home;Portal Recall" });
        var graph = new RouteGraph();
        graph.Build(db, maxWalkDistance: 10.0);

        var path = graph.FindShortestPath("Home", "Town");
        var route = graph.ToRoute(path!, "Town");

        Assert.Equal(1, route.StepCount);
        Assert.Equal(RouteStepKind.Recall, route.Steps[0].Kind);
        Assert.Equal("Portal Recall", route.Steps[0].Via);
    }

    [Fact]
    public void ToRoute_EmptyPath_ReturnsSingleStepRoute()
    {
        var (_, graph) = CreateGraph("A;0;0");

        var path = graph.FindShortestPath("A", "A");
        var route = graph.ToRoute(path!, "A");

        Assert.Equal("A", route.Destination);
        Assert.Equal(1, route.StepCount);
        Assert.Equal("Arrived", route.Steps[0].Via);
    }

    // ── Edge cases ──────────────────────────────────────────────────

    [Fact]
    public void Build_EmptyDatabase_ProducesEmptyGraph()
    {
        var db = new LocationDatabase();
        var graph = new RouteGraph();
        graph.Build(db);

        Assert.Equal(0, graph.NodeCount);
    }

    [Fact]
    public void Build_NoEdges_SingleNodeHasNoConnections()
    {
        var (_, graph) = CreateGraph("Alone;0;0");

        var idx = graph.GetNodeIndex("Alone");
        Assert.Empty(graph.GetEdges(idx));
    }

    [Fact]
    public void FindShortestPath_NotBuilt_Throws()
    {
        var graph = new RouteGraph();
        Assert.Throws<InvalidOperationException>(() => graph.FindShortestPath(0, 0));
    }

    [Fact]
    public void GetNodeIndex_NotBuilt_DoesNotThrow()
    {
        var graph = new RouteGraph();
        // Should not throw — just returns -1 for anything
        Assert.Equal(-1, graph.GetNodeIndex("Anything"));
    }

    [Fact]
    public void Build_RespectsCustomWalkDistance()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(new[] { "A;0;0", "B;20;0", "C;30;0" });
        var graph = new RouteGraph();

        // With maxWalkDistance = 25, A↔B and B↔C connect, but A↔C don't
        graph.Build(db, maxWalkDistance: 25.0);

        var aIdx = graph.GetNodeIndex("A");
        var bIdx = graph.GetNodeIndex("B");
        var cIdx = graph.GetNodeIndex("C");

        Assert.Contains(graph.GetEdges(aIdx), e => e.ToIndex == bIdx);
        Assert.Contains(graph.GetEdges(bIdx), e => e.ToIndex == cIdx);
        Assert.DoesNotContain(graph.GetEdges(aIdx), e => e.ToIndex == cIdx);
    }
}
