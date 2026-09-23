using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public class CoordinatesTests
{
    [Fact]
    public void Coordinates_Create_StoresValues()
    {
        var coords = new Coordinates(42.1, 33.6);
        Assert.Equal(42.1, coords.NS);
        Assert.Equal(33.6, coords.EW);
    }

    [Fact]
    public void Coordinates_NoCoordinates_IsNaN()
    {
        var coords = Coordinates.NoCoordinates;
        Assert.True(double.IsNaN(coords.NS));
        Assert.True(double.IsNaN(coords.EW));
    }

    [Fact]
    public void Coordinates_DistanceTo_ComputesCorrectly()
    {
        var a = new Coordinates(0, 0);
        var b = new Coordinates(3, 4);
        Assert.Equal(5.0, a.DistanceTo(b), precision: 10);
    }

    [Fact]
    public void Coordinates_DistanceTo_NaNReturnsInfinity()
    {
        var a = Coordinates.NoCoordinates;
        var b = new Coordinates(0, 0);
        Assert.True(double.IsPositiveInfinity(a.DistanceTo(b)));
    }

    [Fact]
    public void Coordinates_AngleTo_ComputesCorrectly()
    {
        var a = new Coordinates(0, 0);
        var b = new Coordinates(1, 0); // Due north
        Assert.Equal(0.0, a.AngleTo(b), precision: 10);

        var c = new Coordinates(0, 1); // Due east
        Assert.Equal(Math.PI / 2, a.AngleTo(c), precision: 10);
    }

    [Fact]
    public void Coordinates_TryParse_FindsCoords()
    {
        Assert.True(Coordinates.TryParse("42.1N 33.6E", out var coords));
        Assert.Equal(42.1, coords.NS, precision: 4);
        Assert.Equal(33.6, coords.EW, precision: 4);
    }

    [Fact]
    public void Coordinates_TryParse_SouthWest()
    {
        Assert.True(Coordinates.TryParse("10.5S 20.3W", out var coords));
        Assert.Equal(-10.5, coords.NS, precision: 4);
        Assert.Equal(-20.3, coords.EW, precision: 4);
    }

    [Fact]
    public void Coordinates_TryParse_NoCoordsReturnsFalse()
    {
        Assert.False(Coordinates.TryParse("Hello World", out _));
    }

    [Fact]
    public void Coordinates_ToString_FormatsCorrectly()
    {
        var coords = new Coordinates(42.1, 33.6);
        Assert.Equal("42.1N, 33.6E", coords.ToString());
    }

    [Fact]
    public void Coordinates_ToString_SouthWest()
    {
        var coords = new Coordinates(-10.5, -20.3);
        Assert.Equal("10.5S, 20.3W", coords.ToString());
    }

    [Fact]
    public void Coordinates_NoCoordinates_ToString_ReturnsNone()
    {
        Assert.Equal(Coordinates.NoCoordinatesString, Coordinates.NoCoordinates.ToString());
    }

    [Fact]
    public void Coordinates_Equality_SameValues()
    {
        var a = new Coordinates(42.1, 33.6);
        var b = new Coordinates(42.1, 33.6);
        Assert.Equal(a, b);
        Assert.True(a == b);
        Assert.False(a != b);
    }

    [Fact]
    public void Coordinates_Equality_DifferentValues()
    {
        var a = new Coordinates(42.1, 33.6);
        var b = new Coordinates(10.0, 20.0);
        Assert.NotEqual(a, b);
        Assert.True(a != b);
        Assert.False(a == b);
    }

    [Fact]
    public void Coordinates_Equality_NoCoordinates()
    {
        var a = Coordinates.NoCoordinates;
        var b = Coordinates.NoCoordinates;
        Assert.Equal(a, b);
    }

    [Fact]
    public void Coordinates_Round_RoundsCorrectly()
    {
        var coords = new Coordinates(42.12345, 33.67890);
        var rounded = Coordinates.Round(coords, 2);
        Assert.Equal(42.12, rounded.NS, precision: 2);
        Assert.Equal(33.68, rounded.EW, precision: 2);
    }

    [Fact]
    public void Coordinates_LandcellConversion_ProducesValidCoords()
    {
        var coords = new Coordinates(0x00E70000, 12, 12);
        Assert.False(double.IsNaN(coords.NS));
        Assert.False(double.IsNaN(coords.EW));
    }
}
