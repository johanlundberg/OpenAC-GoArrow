using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class TravelDistanceTests
{
    [Theory]
    [InlineData(0, "0 m")]
    [InlineData(0.01, "2.4 m")]
    [InlineData(0.13, "31 m")]
    [InlineData(1000.0 / 240.0, "1000 m")]
    [InlineData(5, "1.2 km")]
    public void DisplaysCoordinateDistanceInMetersOrKilometers(
        double coordinateUnits,
        string expected
    )
    {
        Assert.Equal(expected, TravelDistance.Format(coordinateUnits));
    }

    [Fact]
    public void RouteInstructionsNameOnlyTheActionAndTarget()
    {
        var start = new Location("Current Location", 0, 0);
        var shoushi = new Location("Town Network Portal(Shoushi)", 0, 0.13);
        var shoushiArrival = new Location("Town Network Portal(Shoushi) arrival (9299)", 40, 40);
        var sawatoPortal = new Location("Town Network (E R 5) to Sawato", 40, 40);
        var sawatoArrival = new Location("Town Network (E R 5) to Sawato arrival (9350)", 80, 80);
        var sawato = new Location("Sawato", 80, 80.57);
        var route = new Route("Sawato");
        route.AddTravelStep(start, shoushi, "Walk to start");
        route.AddPortalStep(shoushi, shoushiArrival, shoushi.Name);
        route.AddTravelStep(shoushiArrival, sawatoPortal, "Walk");
        route.AddPortalStep(sawatoPortal, sawatoArrival, sawatoPortal.Name);
        route.AddTravelStep(sawatoArrival, sawato, "Walk");

        Assert.Equal(
            new[]
            {
                "Walk: Town Network Portal(Shoushi) (31 m)",
                "Portal: Town Network Portal(Shoushi)",
                "Walk: Town Network (E R 5) to Sawato (0 m)",
                "Portal: Town Network (E R 5) to Sawato",
                "Walk: Sawato (137 m)",
            },
            route.Steps.Select(step => step.ToString())
        );
    }
}
