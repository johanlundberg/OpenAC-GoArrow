using System.Xml;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public class LocationMetadataTests
{
    [Fact]
    public void IndoorCellPositionRoundTripsInNamedLocationXml()
    {
        var document = new XmlDocument();
        document.LoadXml("""
            <loc name="Lower Chamber" type="Dungeon" cellId="0x12340122" x="35" y="50" z="-6">A note</loc>
            """);

        var location = Location.FromXml(document.DocumentElement!);
        Assert.NotNull(location.IndoorPosition);
        Assert.Equal((uint)0x12340122, location.IndoorPosition.Value.CellId);
        Assert.Equal("A note", location.Notes);

        document.LoadXml(location.ToXml());
        var copy = Location.FromXml(document.DocumentElement!);
        Assert.Equal(location.IndoorPosition, copy.IndoorPosition);
        Assert.Equal("A note", copy.Notes);
    }

    [Fact]
    public void CompactXml_PreservesLocationMetadata()
    {
        var document = new XmlDocument();
        document.LoadXml(
            "<loc id='123' name='Dungeon Entrance' type='Dungeon' "
            + "NS='10.5' EW='20.5' exitNS='11.5' exitEW='21.5' "
            + "dungeonId='ABCD' use='false' retired='true' customized='true' icon='00FF00FF'>"
            + "A useful description</loc>");

        var location = Location.FromXml(document.DocumentElement!);

        Assert.Equal(123, location.Id);
        Assert.Equal("Dungeon Entrance", location.Name);
        Assert.Equal(LocationType.Dungeon, location.Type);
        Assert.Equal(10.5, location.NS);
        Assert.Equal(20.5, location.EW);
        Assert.Equal(11.5, location.ExitCoords.NS);
        Assert.Equal(21.5, location.ExitCoords.EW);
        Assert.Equal(0xABCD, location.DungeonId);
        Assert.False(location.UseInRouteFinding);
        Assert.True(location.IsRetired);
        Assert.True(location.IsCustomized);
        Assert.Equal(0x00FF00FF, location.SpecializedIcon);
        Assert.Equal("A useful description", location.Notes);
    }

    [Fact]
    public void WarcryXml_ConvertsCoordinatesAndPreservesMetadata()
    {
        var document = new XmlDocument();
        document.LoadXml(
            "<location>"
            + "<id>456</id><latitude>28.200</latitude><longitude>13.900</longitude>"
            + "<name>North Outpost</name><type>Outpost</type>"
            + "<arrival_latitude>28.100</arrival_latitude><arrival_longitude>13.800</arrival_longitude>"
            + "<description>Atlas description</description><dungeon_id>1A</dungeon_id>"
            + "<retired>Y</retired></location>");

        var location = Location.FromXmlWarcry(document.DocumentElement!);

        Assert.Equal(456, location.Id);
        Assert.Equal(LocationType.Outpost, location.Type);
        Assert.Equal(-28.2, location.NS, precision: 3);
        Assert.Equal(13.9, location.EW, precision: 3);
        Assert.Equal(-28.1, location.ExitCoords.NS, precision: 3);
        Assert.Equal(13.8, location.ExitCoords.EW, precision: 3);
        Assert.Equal(0x1A, location.DungeonId);
        Assert.True(location.IsRetired);
        Assert.False(location.UseInRouteFinding);
    }

    [Fact]
    public void LocationDatabase_LoadsWarcryAtlasLocations()
    {
        var db = new LocationDatabase();
        db.LoadLocationsXml(
            "<atlas><location><id>1</id><latitude>1</latitude><longitude>2</longitude>"
            + "<name>Atlas Point</name><type>Landmark</type><retired>N</retired></location></atlas>");

        var location = db.FindLocation("Atlas Point");

        Assert.NotNull(location);
        Assert.Equal(1, location.Id);
        Assert.Equal(LocationType.Landmark, location.Type);
        Assert.Equal(-1, location.NS);
        Assert.Equal(2, location.EW);
    }

    [Fact]
    public void Location_CompactXml_RoundTripsMetadata()
    {
        var source = new Location(
            42,
            "Town",
            LocationType.Town,
            new Coordinates(10, 20),
            "Notes",
            dungeonId: 7,
            exitCoords: new Coordinates(11, 21))
        {
            IsCustomized = true,
            IsFavorite = true,
            IsRetired = true,
            UseInRouteFinding = false,
            SpecializedIcon = 0x1234,
        };

        var document = new XmlDocument();
        document.LoadXml(source.ToXml());
        var copy = Location.FromXml(document.DocumentElement!);

        Assert.Equal(source.Id, copy.Id);
        Assert.Equal(source.Name, copy.Name);
        Assert.Equal(source.Type, copy.Type);
        Assert.Equal(source.Coords, copy.Coords);
        Assert.Equal(source.ExitCoords, copy.ExitCoords);
        Assert.Equal(source.DungeonId, copy.DungeonId);
        Assert.Equal(source.Notes, copy.Notes);
        Assert.Equal(source.IsCustomized, copy.IsCustomized);
        Assert.Equal(source.IsFavorite, copy.IsFavorite);
        Assert.Equal(source.IsRetired, copy.IsRetired);
        Assert.Equal(source.UseInRouteFinding, copy.UseInRouteFinding);
        Assert.Equal(source.SpecializedIcon, copy.SpecializedIcon);
    }
}
