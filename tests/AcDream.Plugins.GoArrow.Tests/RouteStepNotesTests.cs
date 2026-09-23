using AcDream.Plugin.Tests.Fixtures;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class RouteStepNotesTests
{
    [Fact]
    public void RouteListMarksStepsWhoseDetailsHaveNotes()
    {
        var host = new FakePluginHost();
        var settings = new GoArrowSettings();
        var database = new LocationDatabase();
        var destination = new GoArrowDestination(settings, database, new RouteFinder(database));
        var target = new Location(1, "Library", LocationType.Unknown,
            new Coordinates(0, 1), "Read the plaque by the entrance.");
        destination.SetDestination(target);
        destination.CalculateRoute(new Location("Start", 0, 0));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        var panel = new GoArrowPanel(host, new GoArrowPlugin(), settings, destination, navigator);

        Assert.EndsWith(" [notes]", Assert.Single(panel.RouteSteps));
        panel.SelectRouteStepAction(0);
        Assert.DoesNotContain("[notes]", panel.DetailsInstructionText);
        Assert.Contains(panel.DetailsNotesLines, line => line.Contains("plaque"));

        target.Notes = "   ";
        Assert.DoesNotContain("[notes]", Assert.Single(panel.RouteSteps));
    }
}
