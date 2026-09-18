using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public class RouteTests
{
    [Fact]
    public void Route_Create_HasCorrectDestination()
    {
        var route = new Route("Holtburg");
        Assert.Equal("Holtburg", route.Destination);
    }

    [Fact]
    public void Route_AddTravelStep_IncreasesStepCount()
    {
        var route = new Route("Holtburg");
        var from = new Location("Start", 0, 0);
        var to = new Location("Holtburg", 42.1, 33.6);

        route.AddTravelStep(from, to, "Walk");

        Assert.Equal(1, route.StepCount);
    }

    [Fact]
    public void Route_AddTravelStep_CalculatesDistance()
    {
        var route = new Route("Dest");
        var from = new Location("Start", 0, 0);
        var to = new Location("Dest", 3, 4);

        route.AddTravelStep(from, to, "Walk");

        Assert.True(route.TotalDistance > 0);
    }

    [Fact]
    public void Route_AddPortalStep_CountsPortal()
    {
        var route = new Route("Town");
        var from = new Location("Outside", 10, 10);
        var to = new Location("Town", 10, 11);

        route.AddPortalStep(from, to, "Town Portal");

        Assert.Equal(1, route.PortalCount);
        Assert.Equal(1, route.StepCount);
    }

    [Fact]
    public void Route_Clear_ResetsAll()
    {
        var route = new Route("Dest");
        route.AddTravelStep(new Location("A", 0, 0), new Location("B", 1, 1), "Walk");
        route.Clear();

        Assert.Equal(0, route.StepCount);
        Assert.Equal(0, route.TotalDistance);
        Assert.Equal(0, route.PortalCount);
    }

    [Fact]
    public void Route_Steps_AreReadOnly()
    {
        var route = new Route("Dest");
        Assert.Empty(route.Steps);
    }

    [Fact]
    public void RouteStep_ToString_Travel()
    {
        var step = new RouteStep(
            RouteStepKind.Travel,
            new Location("A", 0, 0),
            new Location("B", 1, 1),
            1.414, "Walk");

        var str = step.ToString();
        Assert.Contains("Walk", str);
        Assert.Contains("A", str);
        Assert.Contains("B", str);
    }

    [Fact]
    public void RouteStep_ToString_Portal()
    {
        var step = new RouteStep(
            RouteStepKind.Portal,
            new Location("A", 0, 0),
            new Location("Town", 10, 10),
            0, "Portal Device");

        var str = step.ToString();
        Assert.Contains("Portal", str);
        Assert.Contains("Portal Device", str);
    }

    [Fact]
    public void RouteStep_ToString_Recall()
    {
        var step = new RouteStep(
            RouteStepKind.Recall,
            new Location("A", 0, 0),
            new Location("LS", 50, 50),
            0, "Primary Portal Recall");

        var str = step.ToString();
        Assert.Contains("Recall", str);
        Assert.Contains("Primary Portal Recall", str);
    }
}