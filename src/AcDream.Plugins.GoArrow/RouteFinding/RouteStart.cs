using System.Globalization;
using System.Text.RegularExpressions;

namespace AcDream.Plugins.GoArrow.RouteFinding;

/// <summary>
/// A route start — the beginning segment of a known route.
/// Ported from GoArrow RouteStart.cs — Decal removed.
/// </summary>
public class RouteStart : IEquatable<RouteStart>
{
    private static readonly Regex LoadRegex = new(
        @"^\s*(?<dest>[^;]+)\s*;\s*(?<from>[^;]+)\s*;\s*(?<via>.*)$",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    /// <summary>The destination this route start goes to.</summary>
    public string Destination { get; set; } = string.Empty;

    /// <summary>The origin / starting point name.</summary>
    public string From { get; set; } = string.Empty;

    /// <summary>How to start (e.g., a recall or portal).</summary>
    public string Via { get; set; } = string.Empty;

    public RouteStart() { }

    public RouteStart(string destination, string from, string via)
    {
        Destination = destination;
        From = from;
        Via = via;
    }

    public override string ToString()
    {
        return $"{Destination};{From};{Via}";
    }

    public static RouteStart FromCsvLine(string line)
    {
        var m = LoadRegex.Match(line);
        if (!m.Success)
            return new RouteStart { Destination = line.Trim() };

        return new RouteStart(
            m.Groups["dest"].Value.Trim(),
            m.Groups["from"].Value.Trim(),
            m.Groups["via"].Value.Trim());
    }

    public override bool Equals(object? obj) => obj is RouteStart other && Equals(other);

    public bool Equals(RouteStart? other)
    {
        if (other is null) return false;
        return string.Equals(Destination, other.Destination, StringComparison.OrdinalIgnoreCase)
            && string.Equals(From, other.From, StringComparison.OrdinalIgnoreCase);
    }

    public override int GetHashCode() =>
        HashCode.Combine(
            StringComparer.OrdinalIgnoreCase.GetHashCode(Destination),
            StringComparer.OrdinalIgnoreCase.GetHashCode(From));

    public static bool operator ==(RouteStart? a, RouteStart? b)
    {
        if (a is null && b is null) return true;
        if (a is null || b is null) return false;
        return a.Equals(b);
    }

    public static bool operator !=(RouteStart? a, RouteStart? b) => !(a == b);
}