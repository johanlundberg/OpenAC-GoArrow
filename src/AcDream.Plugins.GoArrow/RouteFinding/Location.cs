using System.Globalization;
using System.Text.RegularExpressions;
using System.Xml;
using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow.RouteFinding;

/// <summary>
/// A named point in the GoArrow location database.
///
/// The original GoArrow model contains more than a name and coordinates. This
/// port retains identifiers, semantic type, notes, dungeon/exit metadata,
/// retirement state, customization/favorite state, and route-finding policy.
/// </summary>
public sealed class Location : IEquatable<Location>, IComparable<Location>
{
    private static readonly Regex CsvRegex = new(
        @"^\s*(?<name>[^;]*);\s*(?<NS>.+?)\s*;\s*(?<EW>.+?)\s*$",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    private static int _nextCustomId = 2_000_000_000;
    private static int _nextInternalId = -2;

    public Location() { }

    public Location(string name)
    {
        Name = name;
    }

    public Location(string name, double ns, double ew)
        : this(0, name, LocationType.Unknown, new Coordinates(ns, ew), string.Empty)
    {
    }

    public Location(string name, Coordinates coords)
        : this(0, name, LocationType.Unknown, coords, string.Empty)
    {
    }

    public Location(
        int id,
        string name,
        LocationType type,
        Coordinates coords,
        string notes,
        int dungeonId = 0,
        Coordinates? exitCoords = null)
    {
        Id = id;
        Name = name ?? string.Empty;
        Type = type;
        Coords = coords;
        Notes = notes ?? string.Empty;
        DungeonId = dungeonId;
        ExitCoords = exitCoords ?? Coordinates.NoCoordinates;

        if (id >= _nextCustomId)
            _nextCustomId = checked(id + 1);
    }

    /// <summary>Stable source/database identifier.</summary>
    public int Id { get; set; }

    /// <summary>Display name.</summary>
    public string Name { get; set; } = string.Empty;

    /// <summary>Semantic location category.</summary>
    public LocationType Type { get; set; } = LocationType.Unknown;

    /// <summary>Primary location coordinates.</summary>
    public Coordinates Coords { get; set; } = Coordinates.NoCoordinates;

    /// <summary>Optional arrival/exit coordinates for portals and dungeons.</summary>
    public Coordinates ExitCoords { get; set; } = Coordinates.NoCoordinates;

    /// <summary>Optional dungeon or region identifier.</summary>
    public int DungeonId { get; set; }

    /// <summary>Optional exact indoor cell and floor position for local location data.</summary>
    public PluginNavigationPosition? IndoorPosition { get; set; }

    /// <summary>Human-readable description or source notes.</summary>
    public string Notes { get; set; } = string.Empty;

    /// <summary>Whether the location is marked as a user favorite.</summary>
    public bool IsFavorite { get; set; }

    /// <summary>Whether the location was created or modified by the user.</summary>
    public bool IsCustomized { get; set; }

    /// <summary>Whether the source marked this location retired.</summary>
    public bool IsRetired { get; set; }

    /// <summary>Whether route-finding may use this location as a graph node.</summary>
    public bool UseInRouteFinding { get; set; } = true;

    /// <summary>Optional host/icon identifier retained from the original database.</summary>
    public int SpecializedIcon { get; set; }

    /// <summary>Compatibility coordinate accessors used by earlier port code.</summary>
    public double NS
    {
        get => Coords.NS;
        set => Coords = new Coordinates(value, EW);
    }

    public double EW
    {
        get => Coords.EW;
        set => Coords = new Coordinates(NS, value);
    }

    public bool HasCoordinates => Coords != Coordinates.NoCoordinates;
    public bool HasExitCoords => ExitCoords != Coordinates.NoCoordinates;
    public bool IsInternalLocation => IsInternalId(Id);

    public double DistanceTo(Location other) => Coords.DistanceTo(other.Coords);
    public double AngleTo(Location other) => Coords.AngleTo(other.Coords);

    public bool TypeMatches(LocationType type) =>
        Type == type || (Type & type) != 0;

    public static bool IsInternalId(int id) => id < 0;

    public static int GetNextCustomId() => checked(_nextCustomId++);
    public static int GetNextInternalId() => checked(_nextInternalId--);

    public static Location FromCsvLine(string line)
    {
        Match match = CsvRegex.Match(line);
        if (!match.Success)
            return new Location(line.Trim());

        return new Location(
            match.Groups["name"].Value.Trim(),
            double.Parse(match.Groups["NS"].Value, CultureInfo.InvariantCulture),
            double.Parse(match.Groups["EW"].Value, CultureInfo.InvariantCulture));
    }

    /// <summary>
    /// Reads the compact GoArrow location format:
    /// &lt;loc id="..." name="..." type="..." NS="..." EW="..." ...&gt;notes&lt;/loc&gt;.
    /// Also accepts the current port's element-based Location/Coords format.
    /// </summary>
    public static Location FromXml(XmlElement element, bool useInternalId = false)
    {
        bool compact = element.HasAttribute("NS") || element.Name.Equals("loc", StringComparison.OrdinalIgnoreCase);
        int id = useInternalId
            ? GetNextInternalId()
            : ParseInt(GetAttribute(element, "id"), 0);
        string name = GetAttribute(element, "name")
            ?? ChildText(element, "Name")
            ?? string.Empty;
        LocationType type = ParseLocationType(GetAttribute(element, "type") ?? ChildText(element, "Type"));

        Coordinates coords;
        Coordinates exitCoords = Coordinates.NoCoordinates;
        if (compact)
        {
            coords = new Coordinates(
                ParseDouble(GetAttribute(element, "NS")),
                ParseDouble(GetAttribute(element, "EW")));
            if (element.HasAttribute("exitNS") || element.HasAttribute("exitEW"))
            {
                exitCoords = new Coordinates(
                    ParseDouble(GetAttribute(element, "exitNS")),
                    ParseDouble(GetAttribute(element, "exitEW")));
            }
        }
        else
        {
            XmlElement? coordsElement = element.SelectSingleNode("Coords") as XmlElement;
            coords = coordsElement is null
                ? new Coordinates(ParseDouble(ChildText(element, "NS")), ParseDouble(ChildText(element, "EW")))
                : new Coordinates(
                    ParseDouble(coordsElement.GetAttribute("NS")),
                    ParseDouble(coordsElement.GetAttribute("EW")));

            XmlElement? exitElement = element.SelectSingleNode("ExitCoords") as XmlElement;
            if (exitElement is not null)
            {
                exitCoords = new Coordinates(
                    ParseDouble(exitElement.GetAttribute("NS")),
                    ParseDouble(exitElement.GetAttribute("EW")));
            }
        }

        PluginNavigationPosition? indoorPosition = ParseIndoorPosition(element);
        if (indoorPosition is { } indoor && (!double.IsFinite(coords.NS) || !double.IsFinite(coords.EW)))
            coords = new Coordinates(indoor.NorthSouth, indoor.EastWest);

        var location = new Location(
            id,
            name,
            type,
            coords,
            element.InnerText.Trim(),
            ParseHexInt(GetAttribute(element, "dungeonId") ?? ChildText(element, "DungeonId"), 0),
            exitCoords);

        location.Notes = GetAttribute(element, "notes") ?? element.InnerText.Trim();
        location.IsCustomized = ParseBool(GetAttribute(element, "customized"), false);
        location.IsFavorite = ParseBool(GetAttribute(element, "favorite"), false);
        location.IsRetired = ParseBool(GetAttribute(element, "retired"), false);
        location.UseInRouteFinding = ParseBool(
            GetAttribute(element, "use") ?? GetAttribute(element, "useInRouteFinding"), true);
        location.SpecializedIcon = ParseHexInt(GetAttribute(element, "icon"), 0);
        location.IndoorPosition = indoorPosition;
        return location;
    }

    /// <summary>
    /// Reads the Crossroads of Dereth/Warcry Atlas location format. The
    /// downloader is intentionally separate; this parser preserves the data
    /// needed when an external provider is added later.
    /// </summary>
    public static Location FromXmlWarcry(XmlElement element)
    {
        int id = ParseInt(ChildText(element, "id"), 0);
        string name = ChildText(element, "name") ?? string.Empty;
        double latitude = ParseDouble(ChildText(element, "latitude"));
        double longitude = ParseDouble(ChildText(element, "longitude"));

        // Atlas latitude uses the opposite NS sign from GoArrow coordinates.
        var coords = new Coordinates(-latitude, longitude);
        string? arrivalLatitude = ChildText(element, "arrival_latitude");
        string? arrivalLongitude = ChildText(element, "arrival_longitude");
        Coordinates exit = Coordinates.NoCoordinates;
        double arrivalNS = ParseDouble(arrivalLatitude);
        double arrivalEW = ParseDouble(arrivalLongitude);
        if (double.IsFinite(arrivalNS)
            && double.IsFinite(arrivalEW)
            && (arrivalNS != 0 || arrivalEW != 0))
        {
            exit = new Coordinates(-arrivalNS, arrivalEW);
        }

        var location = new Location(
            id,
            name,
            ParseLocationType(ChildText(element, "type")),
            coords,
            ChildText(element, "description") ?? string.Empty,
            ParseHexInt(ChildText(element, "dungeon_id"), 0),
            exit);
        location.IsRetired = string.Equals(ChildText(element, "retired"), "Y", StringComparison.OrdinalIgnoreCase);
        location.UseInRouteFinding = !location.IsRetired;
        return location;
    }

    private static PluginNavigationPosition? ParseIndoorPosition(XmlElement element)
    {
        XmlElement source = element.SelectSingleNode("Position") as XmlElement ?? element;
        string? cellText = GetAttribute(source, "cellId");
        if (string.IsNullOrWhiteSpace(cellText))
            return null;
        string hex = cellText.StartsWith("0x", StringComparison.OrdinalIgnoreCase)
            ? cellText[2..] : cellText;
        if (!uint.TryParse(hex, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out uint cellId)
            || (cellId & 0xFFFFu) <= 0x40u
            || !double.TryParse(GetAttribute(source, "x"), NumberStyles.Float, CultureInfo.InvariantCulture, out double x)
            || !double.TryParse(GetAttribute(source, "y"), NumberStyles.Float, CultureInfo.InvariantCulture, out double y)
            || !double.TryParse(GetAttribute(source, "z"), NumberStyles.Float, CultureInfo.InvariantCulture, out double z)
            || !double.IsFinite(x) || !double.IsFinite(y) || !double.IsFinite(z))
            throw new FormatException($"Location '{GetAttribute(element, "name")}' has an invalid indoor position.");
        uint blockX = (cellId >> 24) & 0xFFu;
        uint blockY = (cellId >> 16) & 0xFFu;
        return new PluginNavigationPosition(
            cellId,
            (((double)blockX - 127d) * 192d + x - 84d) / 240d,
            (((double)blockY - 127d) * 192d + y - 84d) / 240d,
            z / 240d, 0f, false);
    }

    /// <summary>Serializes the complete compact GoArrow location model.</summary>
    public string ToXml()
    {
        var document = new XmlDocument();
        XmlElement element = document.CreateElement("loc");
        element.SetAttribute("id", Id.ToString(CultureInfo.InvariantCulture));
        element.SetAttribute("name", Name);
        element.SetAttribute("type", Type.ToString());
        element.SetAttribute("NS", NS.ToString(CultureInfo.InvariantCulture));
        element.SetAttribute("EW", EW.ToString(CultureInfo.InvariantCulture));
        if (HasExitCoords)
        {
            element.SetAttribute("exitNS", ExitCoords.NS.ToString(CultureInfo.InvariantCulture));
            element.SetAttribute("exitEW", ExitCoords.EW.ToString(CultureInfo.InvariantCulture));
        }
        if (DungeonId != 0)
            element.SetAttribute("dungeonId", DungeonId.ToString(CultureInfo.InvariantCulture));
        if (IndoorPosition is { } indoor)
        {
            var local = PluginDungeonFloorplan.ToLandblockLocal(indoor);
            element.SetAttribute("cellId", $"0x{indoor.CellId:X8}");
            element.SetAttribute("x", local.X.ToString("R", CultureInfo.InvariantCulture));
            element.SetAttribute("y", local.Y.ToString("R", CultureInfo.InvariantCulture));
            element.SetAttribute("z", local.Z.ToString("R", CultureInfo.InvariantCulture));
        }
        if (!UseInRouteFinding)
            element.SetAttribute("use", bool.FalseString);
        if (IsCustomized)
            element.SetAttribute("customized", bool.TrueString);
        if (IsFavorite)
            element.SetAttribute("favorite", bool.TrueString);
        if (IsRetired)
            element.SetAttribute("retired", bool.TrueString);
        if (SpecializedIcon != 0)
            element.SetAttribute("icon", SpecializedIcon.ToString("X8", CultureInfo.InvariantCulture));
        element.InnerText = Notes;
        document.AppendChild(element);
        return document.OuterXml;
    }

    public override string ToString() => HasCoordinates
        ? $"{Name} [{Coords}]"
        : Name;

    public bool Equals(Location? other)
    {
        if (other is null)
            return false;
        if (Id != 0 && other.Id != 0)
            return Id == other.Id;
        return string.Equals(Name, other.Name, StringComparison.OrdinalIgnoreCase)
            && Coords == other.Coords;
    }

    public override bool Equals(object? obj) => obj is Location other && Equals(other);
    public override int GetHashCode() => Id != 0
        ? Id
        : HashCode.Combine(StringComparer.OrdinalIgnoreCase.GetHashCode(Name), Coords);

    public int CompareTo(Location? other)
    {
        if (other is null)
            return 1;
        int result = StringComparer.OrdinalIgnoreCase.Compare(Name, other.Name);
        return result != 0 ? result : Id.CompareTo(other.Id);
    }

    public static bool operator ==(Location? left, Location? right) =>
        ReferenceEquals(left, right) || (left is not null && left.Equals(right));

    public static bool operator !=(Location? left, Location? right) => !(left == right);

    private static string? GetAttribute(XmlElement element, string name) =>
        element.HasAttribute(name) ? element.GetAttribute(name) : null;

    private static string? ChildText(XmlElement element, string name) =>
        element.SelectSingleNode(name)?.InnerText;

    private static double ParseDouble(string? value) =>
        double.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out double result)
            ? result
            : double.NaN;

    private static int ParseInt(string? value, int fallback)
    {
        return int.TryParse(value, NumberStyles.Integer, CultureInfo.InvariantCulture, out int result)
            ? result
            : fallback;
    }

    private static int ParseHexInt(string? value, int fallback)
    {
        return int.TryParse(value, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out int hexValue)
            ? hexValue
            : fallback;
    }

    private static bool ParseBool(string? value, bool fallback) =>
        bool.TryParse(value, out bool result) ? result : fallback;

    private static LocationType ParseLocationType(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
            return LocationType.Unknown;
        if (Enum.TryParse<LocationType>(value, true, out LocationType type))
            return type;

        return value.Trim().ToLowerInvariant() switch
        {
            "allegiance hall" => LocationType.AllegianceHall,
            "bindstone" => LocationType.Bindstone,
            "dungeon" => LocationType.Dungeon,
            "landmark" => LocationType.Landmark,
            "lifestone" => LocationType.Lifestone,
            "npc" => LocationType.Npc,
            "outpost" => LocationType.Outpost,
            "portal hub" => LocationType.PortalHub,
            "settlement portal" => LocationType.SettlementPortal,
            "town" => LocationType.Town,
            "town portal" => LocationType.TownPortal,
            "underground portal" => LocationType.UndergroundPortal,
            "vendor" => LocationType.Vendor,
            "village" or "settlement" => LocationType.Village,
            "wilderness portal" => LocationType.WildernessPortal,
            "portal" => LocationType.Portal,
            _ => LocationType.Unknown,
        };
    }
}

/// <summary>Semantic categories used by the original GoArrow location database.</summary>
[Flags]
public enum LocationType : uint
{
    Unknown = 0,
    AllegianceHall = 1u << 0,
    Bindstone = 1u << 1,
    Dungeon = 1u << 2,
    Landmark = 1u << 3,
    Lifestone = 1u << 4,
    Npc = 1u << 5,
    NPC = Npc,
    Outpost = 1u << 6,
    Portal = 1u << 7,
    PortalHub = 1u << 8,
    SettlementPortal = 1u << 9,
    Village = 1u << 10,
    Town = 1u << 11,
    TownPortal = 1u << 12,
    UndergroundPortal = 1u << 13,
    Vendor = 1u << 14,
    WildernessPortal = 1u << 15,
    AnyPortal = Portal | SettlementPortal | TownPortal | UndergroundPortal | WildernessPortal,
    Any = uint.MaxValue,
}
