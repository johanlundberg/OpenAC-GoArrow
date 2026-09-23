using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public class RouteStartTests
{
    [Fact]
    public void RouteStart_Create_StoresValues()
    {
        var rs = new RouteStart("Holtburg", "APortal", "Portal Spell");
        Assert.Equal("Holtburg", rs.Destination);
        Assert.Equal("APortal", rs.From);
        Assert.Equal("Portal Spell", rs.Via);
    }

    [Fact]
    public void RouteStart_FromCsvLine_ParsesCorrectly()
    {
        var rs = RouteStart.FromCsvLine("Holtburg;APortal;Portal Spell");
        Assert.Equal("Holtburg", rs.Destination);
        Assert.Equal("APortal", rs.From);
        Assert.Equal("Portal Spell", rs.Via);
    }

    [Fact]
    public void RouteStart_FromCsvLine_NoVia()
    {
        var rs = RouteStart.FromCsvLine("Holtburg;APortal;");
        Assert.Equal("Holtburg", rs.Destination);
        Assert.Equal("APortal", rs.From);
        Assert.Empty(rs.Via);
    }

    [Fact]
    public void RouteStart_Equality_SameDestinationAndFrom()
    {
        var a = new RouteStart("Holtburg", "LS", "Portal");
        var b = new RouteStart("Holtburg", "LS", "Portal");
        Assert.Equal(a, b);
    }

    [Fact]
    public void RouteStart_Equality_CaseInsensitive()
    {
        var a = new RouteStart("Holtburg", "LS", "Portal");
        var b = new RouteStart("holtburg", "ls", "Portal");
        Assert.Equal(a, b);
    }

    [Fact]
    public void RouteStart_Equality_DifferentDestination()
    {
        var a = new RouteStart("Holtburg", "LS", "Portal");
        var b = new RouteStart("Shoushi", "LS", "Portal");
        Assert.NotEqual(a, b);
    }

    [Fact]
    public void RouteStart_ToString_FormatsCorrectly()
    {
        var rs = new RouteStart("Dest", "From", "Via");
        Assert.Equal("Dest;From;Via", rs.ToString());
    }
}
