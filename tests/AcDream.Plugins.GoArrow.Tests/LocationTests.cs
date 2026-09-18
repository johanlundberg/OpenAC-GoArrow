using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public class LocationTests
{
    [Fact]
    public void Location_Create_StoresValues()
    {
        var loc = new Location("Holtburg", 42.1, 33.6);
        Assert.Equal("Holtburg", loc.Name);
        Assert.Equal(42.1, loc.NS);
        Assert.Equal(33.6, loc.EW);
    }

    [Fact]
    public void Location_FromCsvLine_ParsesCorrectly()
    {
        var loc = Location.FromCsvLine("Holtburg;42.1;33.6");
        Assert.Equal("Holtburg", loc.Name);
        Assert.Equal(42.1, loc.NS);
        Assert.Equal(33.6, loc.EW);
    }

    [Fact]
    public void Location_FromCsvLine_NoCoords_ParsesName()
    {
        var loc = Location.FromCsvLine("Hello World");
        Assert.Equal("Hello World", loc.Name);
        Assert.False(loc.HasCoordinates);
    }

    [Fact]
    public void Location_HasCoordinates_Valid()
    {
        var loc = new Location("Test", 10.0, 20.0);
        Assert.True(loc.HasCoordinates);
    }

    [Fact]
    public void Location_HasCoordinates_NaN()
    {
        var loc = new Location("Test");
        Assert.False(loc.HasCoordinates);
    }

    [Fact]
    public void Location_DistanceTo_ComputesCorrectly()
    {
        var a = new Location("A", 0, 0);
        var b = new Location("B", 3, 4);
        Assert.Equal(5.0, a.DistanceTo(b), precision: 10);
    }

    [Fact]
    public void Location_ToXml_ProducesValidXml()
    {
        var loc = new Location("Test", 10.0, 20.0);
        var xml = loc.ToXml();
        Assert.Contains("Test", xml);
        Assert.Contains("10", xml);
        Assert.Contains("20", xml);
    }

    [Fact]
    public void Location_Equality_SameNameAndCoords()
    {
        var a = new Location("Test", 10.0, 20.0);
        var b = new Location("Test", 10.0, 20.0);
        Assert.Equal(a, b);
    }

    [Fact]
    public void Location_Equality_CaseInsensitive()
    {
        var a = new Location("Test", 10.0, 20.0);
        var b = new Location("test", 10.0, 20.0);
        Assert.Equal(a, b);
    }

    [Fact]
    public void Location_Equality_DifferentCoords()
    {
        var a = new Location("Test", 10.0, 20.0);
        var b = new Location("Test", 30.0, 40.0);
        Assert.NotEqual(a, b);
    }
}