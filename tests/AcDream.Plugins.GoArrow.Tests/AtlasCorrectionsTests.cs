using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class AtlasCorrectionsTests
{
    private const string Atlas = """
        <atlas>
          <location><id>1</id><latitude>0</latitude><longitude>1</longitude>
            <name>Portal</name><type>Wilderness Portal</type>
            <arrival_latitude>-50</arrival_latitude><arrival_longitude>50</arrival_longitude></location>
          <location><id>2</id><latitude>-20</latitude><longitude>20</longitude>
            <name>End</name><type>Town</type></location>
        </atlas>
        """;

    [Fact]
    public void UserArrivalOverridesShippedAndAppliesToLoadedAtlas()
    {
        var db = new LocationDatabase();
        db.LoadLocationsXml(Atlas);
        var shipped = AtlasCorrections.Parse("""{ "Arrivals": [ { "Id": 1, "Arrival": "1N, 1E" } ] }""");
        var user = AtlasCorrections.Parse("""
            // Comments and trailing commas are allowed in the user file.
            { "arrivals": [ { "id": 1, "arrival": "19.9N, 20.0E" }, ] }
            """);
        var errors = new List<string>();

        db.SetCorrections(AtlasCorrections.ArrivalsById([shipped, user], errors), []);

        Assert.Empty(errors);
        Assert.Equal(new Coordinates(19.9, 20), db.FindLocation("Portal")!.ExitCoords);
        var route = new RouteFinder(db).FindRoute(new Location("Player", 0, 0), db.FindLocation("End")!);
        Assert.Contains(route.Steps, step => step.Kind == RouteStepKind.Portal);
    }

    [Fact]
    public void UnreadableArrivalIsReportedNotApplied()
    {
        var errors = new List<string>();
        var corrections = AtlasCorrections.Parse("""{ "Arrivals": [ { "Id": 1, "Arrival": "nowhere" } ] }""");

        Assert.Empty(AtlasCorrections.ArrivalsById([corrections], errors));
        Assert.Single(errors);
    }

    [Fact]
    public void BlockedPortalStepIsNotUsedByLaterSearches()
    {
        var db = new LocationDatabase();
        db.LoadLocationsXml(Atlas);
        db.SetCorrections(new Dictionary<int, Coordinates> { [1] = new(19.9, 20) }, []);
        var finder = new RouteFinder(db);
        var origin = new Location("Player", 0, 0);
        var portalStep = finder.FindRoute(origin, db.FindLocation("End")!).Steps
            .Single(step => step.Kind == RouteStepKind.Portal);
        var blocked = AtlasCorrections.BlockedStep.FromRoute(portalStep, "Portal is gone");
        Assert.True(finder.HasGraphStep(blocked));

        db.SetCorrections(new Dictionary<int, Coordinates> { [1] = new(19.9, 20) }, [blocked]);
        finder.InvalidateGraph();

        Assert.False(finder.HasGraphStep(blocked));
        Assert.DoesNotContain(finder.FindRoute(origin, db.FindLocation("End")!).Steps,
            step => step.Kind == RouteStepKind.Portal);
    }

    [Fact]
    public void BlockedWalkIsBlockedInBothDirections()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(["TownA;0;0", "TownB;0;5"]);
        var finder = new RouteFinder(db);
        var walk = new RouteStep(RouteStepKind.Travel, db.FindLocation("TownA")!,
            db.FindLocation("TownB")!, 5, "Walk");
        var blocked = AtlasCorrections.BlockedStep.FromRoute(walk, "");

        db.SetCorrections(new Dictionary<int, Coordinates>(), [blocked]);
        finder.InvalidateGraph();

        var reverse = AtlasCorrections.BlockedStep.FromRoute(new RouteStep(RouteStepKind.Travel,
            db.FindLocation("TownB")!, db.FindLocation("TownA")!, 5, "Walk"), "");
        Assert.False(finder.HasGraphStep(reverse));
    }

    [Fact]
    public void BlockedStepRoundTripsThroughJson()
    {
        var corrections = new AtlasCorrections();
        corrections.BlockedSteps.Add(new AtlasCorrections.BlockedStep
        {
            Kind = "Portal", From = "Portal", FromId = 1, To = "Portal arrival (1)", Note = "gone",
        });

        var copy = AtlasCorrections.Parse(corrections.ToJson());

        var step = Assert.Single(copy.BlockedSteps);
        Assert.Equal(("Portal", 1, "Portal arrival (1)", "gone"), (step.Kind, step.FromId, step.To, step.Note));
    }

    [Fact]
    public void FirstStartCreatesEditableCorrectionsFileFromShippedCopy()
    {
        var host = new FakePluginHost { HasUiValue = false };
        var plugin = new GoArrowPlugin();

        plugin.Initialize(host);

        Assert.Equal(GoArrowPlugin.ReadEmbeddedText("AtlasCorrections.json"),
            host.PluginStorage.ReadText(GoArrowPlugin.CorrectionsStorageKey));
    }

    [Fact]
    public void UnreadableUserFileIsKeptAndShippedCorrectionsStillApply()
    {
        var host = new FakePluginHost { HasUiValue = false };
        host.PluginStorage.WriteText(GoArrowPlugin.CorrectionsStorageKey, "{ not json");
        var plugin = new GoArrowPlugin();

        plugin.Initialize(host);

        Assert.Equal("{ not json", host.PluginStorage.ReadText(GoArrowPlugin.CorrectionsStorageKey));
        Assert.Contains("could not be used", plugin.LoadCorrections());
    }

    [Fact]
    public void BlockingRouteStepSavesItAndRecalculatesWithoutIt()
    {
        var host = new FakePluginHost { HasUiValue = false };
        host.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(true, false, 1,
            new PluginNavigationPosition(0, 0, 0, 0, 0, true), false, false);
        host.PluginStorage.WriteText("data/warcry-atlas.xml", Atlas);
        host.PluginStorage.WriteText(GoArrowPlugin.CorrectionsStorageKey,
            """{ "Arrivals": [ { "Id": 1, "Arrival": "19.9N, 20.0E" } ] }""");
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();
        Assert.True(plugin.SetDestination("End"));
        host.PluginEvents.RaiseTick(0.1);
        var before = plugin.GetCurrentRouteSteps();
        int index = before.ToList().FindIndex(step => step.StartsWith("Portal:", StringComparison.Ordinal));
        Assert.StartsWith("GoArrow: Blocked step", plugin.BlockRouteStep(index, "Portal is gone"));

        host.PluginEvents.RaiseTick(0.1);

        var saved = AtlasCorrections.Parse(host.PluginStorage.ReadText(GoArrowPlugin.CorrectionsStorageKey)!);
        Assert.Single(saved.BlockedSteps);
        Assert.Single(saved.Arrivals);
        Assert.Equal("Portal is gone", saved.BlockedSteps[0].Note);
        Assert.NotEmpty(plugin.GetCurrentRouteSteps());
        Assert.DoesNotContain(before[index], plugin.GetCurrentRouteSteps());
        plugin.Disable();
    }
}
