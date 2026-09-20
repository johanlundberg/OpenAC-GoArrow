using System.Globalization;
using System.Text.RegularExpressions;
using System.Xml;
using System.Xml.Serialization;

namespace AcDream.Plugins.GoArrow.RouteFinding;

/// <summary>
/// A route from one location to another, optionally requiring a portal device.
/// Ported from GoArrow PortalDevice.cs — Decal removed.
/// </summary>
public class PortalDevice : IEquatable<PortalDevice>
{
    private static readonly Regex LoadRegex = new(
        @"^\s*(?<dest>[^;]+)\s*;\s+(?<via>[^;]+)\s*;\s+(?<island>.*)$",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    /// <summary>The destination location name.</summary>
    public string Destination { get; set; } = string.Empty;

    /// <summary>The portal device (item name) that goes to the destination.</summary>
    public string Via { get; set; } = string.Empty;

    /// <summary>The island / landmass the destination is on.</summary>
    public string Landmass { get; set; } = string.Empty;

    /// <summary>
    /// Optional named location where the portal/device must be used.
    /// Older records omit this field and cannot create a graph portal edge.
    /// </summary>
    public string EntranceLocation { get; set; } = string.Empty;

    /// <summary>Optional named arrival location when it differs from Destination.</summary>
    public string ExitLocation { get; set; } = string.Empty;

    public PortalDevice() { }

    public PortalDevice(
        string destination,
        string via,
        string landmass,
        string entranceLocation = "",
        string exitLocation = "")
    {
        Destination = destination;
        Via = via;
        Landmass = landmass;
        EntranceLocation = entranceLocation ?? string.Empty;
        ExitLocation = exitLocation ?? string.Empty;
    }

    public override string ToString()
    {
        if (string.IsNullOrEmpty(EntranceLocation) && string.IsNullOrEmpty(ExitLocation))
            return $"{Destination};{Via};{Landmass}";
        return $"{Destination};{Via};{Landmass};{EntranceLocation};{ExitLocation}";
    }

    public static PortalDevice FromCsvLine(string line)
    {
        string[] fields = line.Split(';');
        if (fields.Length < 3)
            return new PortalDevice { Destination = line.Trim() };

        return new PortalDevice(
            fields[0].Trim(),
            fields[1].Trim(),
            fields[2].Trim(),
            fields.Length > 3 ? fields[3].Trim() : string.Empty,
            fields.Length > 4 ? fields[4].Trim() : string.Empty);
    }

    public override bool Equals(object? obj) => obj is PortalDevice other && Equals(other);

    public bool Equals(PortalDevice? other)
    {
        if (other is null) return false;
        return string.Equals(Destination, other.Destination, StringComparison.OrdinalIgnoreCase)
            && string.Equals(Via, other.Via, StringComparison.OrdinalIgnoreCase);
    }

    public override int GetHashCode() =>
        HashCode.Combine(
            StringComparer.OrdinalIgnoreCase.GetHashCode(Destination),
            StringComparer.OrdinalIgnoreCase.GetHashCode(Via));

    public static bool operator ==(PortalDevice? a, PortalDevice? b)
    {
        if (a is null && b is null) return true;
        if (a is null || b is null) return false;
        return a.Equals(b);
    }

    public static bool operator !=(PortalDevice? a, PortalDevice? b) => !(a == b);
}