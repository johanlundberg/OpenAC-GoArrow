using AcDream.Plugin.Abstractions;
using AcDream.Plugins.GoArrow;
using AcDream.Plugins.GoArrow.RouteFinding;
using Xunit;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class DestinationTests
{
    [Fact]
    public void CoordinateDestinationRetainsKindAndDisplayText()
    {
        var settings = new GoArrowSettings();
        var database = new LocationDatabase();
        var finder = new RouteFinder(database);
        var destination = new GoArrowDestination(settings, database, finder);

        destination.SetCoordinate(42.1, 33.6, "42.1N 33.6E");

        Assert.Equal(GoArrowDestinationKind.Coordinates, destination.Kind);
        Assert.Equal("42.1N 33.6E", destination.CoordinateText);
        Assert.Equal(42.1, destination.TargetLocation!.Coords.NS);
        Assert.Equal(33.6, destination.TargetLocation.Coords.EW);
    }

    [Fact]
    public void ObjectDestinationBecomesUnavailableWhenSelectionIsLost()
    {
        var settings = new GoArrowSettings();
        var database = new LocationDatabase();
        var finder = new RouteFinder(database);
        var destination = new GoArrowDestination(settings, database, finder);
        var obj = new PluginWorldObject(12, 1, "Portal", PluginObjectClass.Portal, 0, 0, 0)
        {
            HasPosition = true,
            Position = new PluginNavigationPosition(1, 4, 5, 0, 0, true),
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable
        };

        Assert.True(destination.SetObject(obj));
        destination.MarkObjectUnavailable();

        Assert.Equal(GoArrowDestinationKind.Object, destination.Kind);
        Assert.True(destination.TargetUnavailable);
        Assert.Equal((uint)12, destination.TargetObjectId);
    }

    [Fact]
    public void RouteGuidancePointsToFirstGraphWaypoint()
    {
        var database = new LocationDatabase();
        database.LoadLocationsCsv(new[]
        {
            "Start;0;0",
            "First;0;10",
            "Finish;10;10"
        });
        var destination = new GoArrowDestination(
            new GoArrowSettings(), database, new RouteFinder(database));
        destination.SetDestination("Finish");

        destination.CalculateRoute(new Location("Current", 0, 0));

        Assert.Equal("First", destination.GetImmediateTarget()?.Name);
        Assert.Equal(90, destination.BearingDegrees, 6);
        Assert.Equal(10, destination.GuidanceDistance, 6);
        Assert.Equal(Math.Sqrt(200), destination.EstimatedDistance, 6);
    }

    [Theory]
    [InlineData(0, 1, 90)]
    [InlineData(-1, 0, 180)]
    [InlineData(0, -1, 270)]
    [InlineData(1, -1, 315)]
    public void BearingUsesClockwiseDegreesFromNorth(double northSouth, double eastWest, double expected)
    {
        var database = new LocationDatabase();
        var destination = new GoArrowDestination(
            new GoArrowSettings(), database, new RouteFinder(database));
        destination.SetCoordinate(northSouth, eastWest, "Target");

        destination.UpdateGuidance(new Location("Current", 0, 0));

        Assert.Equal(expected, destination.BearingDegrees, 6);
    }
}
