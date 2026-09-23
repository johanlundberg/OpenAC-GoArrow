using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public class LocationDatabaseTests
{
    [Fact]
    public void LocationDatabase_LoadLocationsXml_ParsesLocations()
    {
        var db = new LocationDatabase();
        var xml =
            @"<?xml version='1.0' encoding='utf-8'?>
<Locations>
  <Location name='Holtburg'>
    <Coords NS='42.1' EW='33.6' />
  </Location>
  <Location name='Shoushi'>
    <Coords NS='33.4' EW='72.0' />
  </Location>
</Locations>";

        db.LoadLocationsXml(xml);

        Assert.Equal(2, db.LocationCount);

        var holtburg = db.FindLocation("Holtburg");
        Assert.NotNull(holtburg);
        Assert.Equal("Holtburg", holtburg.Name);

        var shoushi = db.FindLocation("shoushi"); // case-insensitive
        Assert.NotNull(shoushi);
        Assert.Equal("Shoushi", shoushi.Name);
    }

    [Fact]
    public void LocationDatabase_LoadLocationsXml_EmptyOnInvalidXml()
    {
        var db = new LocationDatabase();
        db.LoadLocationsXml("<Locations></Locations>");
        Assert.Equal(0, db.LocationCount);
    }

    [Fact]
    public void LocationDatabase_LoadLocationsCsv_ParsesLines()
    {
        var db = new LocationDatabase();
        var lines = new[] { "Holtburg;42.1;33.6", "Shoushi;33.4;72.0", "# This is a comment", "" };

        db.LoadLocationsCsv(lines);

        Assert.Equal(2, db.LocationCount);
    }

    [Fact]
    public void LocationDatabase_SearchLocations_FindsBySubstring()
    {
        var db = new LocationDatabase();
        var lines = new[] { "Holtburg;42.1;33.6", "Shoushi;33.4;72.0" };

        db.LoadLocationsCsv(lines);

        var results = db.SearchLocations("burg");
        Assert.Single(results);
        Assert.Equal("Holtburg", results[0].Name);
    }

    [Fact]
    public void LocationDatabase_FindLocation_ReturnsNullForUnknown()
    {
        var db = new LocationDatabase();
        Assert.Null(db.FindLocation("Nowhere"));
    }

    [Fact]
    public void LocationDatabase_LoadPortalDevicesXml_ParsesDevices()
    {
        var db = new LocationDatabase();
        var xml =
            @"<?xml version='1.0' encoding='utf-8'?>
<PortalDevices>
  <Device Destination='Holtburg' Via='Holtburg Portal Device' Landmass='Dereth' Entrance='Holtburg Plaza' Exit='Holtburg' />
</PortalDevices>";

        db.LoadPortalDevicesXml(xml);

        Assert.Single(db.PortalDevices);
        var ports = db.FindPortalsTo("Holtburg");
        Assert.Single(ports);
        Assert.Equal("Holtburg Portal Device", ports[0].Via);
        Assert.Equal("Holtburg Plaza", ports[0].EntranceLocation);
        Assert.Equal("Holtburg", ports[0].ExitLocation);
    }

    [Fact]
    public void LocationDatabase_LoadRouteStartsXml_ParsesStarts()
    {
        var db = new LocationDatabase();
        var xml =
            @"<?xml version='1.0' encoding='utf-8'?>
<RouteStarts>
  <Start Destination='Holtburg' From='APortal' Via='APortal' />
</RouteStarts>";

        db.LoadRouteStartsXml(xml);

        Assert.Single(db.RouteStarts);
        var from = db.FindRoutesFrom("APortal");
        Assert.Single(from);
        Assert.Equal("Holtburg", from[0].Destination);
    }

    [Fact]
    public void LocationDatabase_Clear_RemovesAll()
    {
        var db = new LocationDatabase();
        db.LoadLocationsCsv(new[] { "Test;10;20" });
        db.LoadPortalDevicesCsv(new[] { "Test;Via;Land" });
        db.LoadRouteStartsCsv(new[] { "Dest;From;Via" });

        db.Clear();

        Assert.Equal(0, db.LocationCount);
        Assert.Empty(db.PortalDevices);
        Assert.Empty(db.RouteStarts);
    }
}
