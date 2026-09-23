using System;
using System.Globalization;
using System.Text.RegularExpressions;

namespace AcDream.Plugins.GoArrow.RouteFinding;

/// <summary>
/// Dereth map coordinates in the standard 240-meter units.
/// Ported from GoArrow Coordinates.cs with Decal dependencies removed.
/// </summary>
public readonly struct Coordinates : IEquatable<Coordinates>
{
    public static readonly Coordinates NoCoordinates = new(double.NaN, double.NaN);
    public const string NoCoordinatesString = "<None>";
    public const string UnknownCoordinatesString = "<Unknown>";

    public double NS { get; }
    public double EW { get; }

    public Coordinates(double ns, double ew)
    {
        NS = ns;
        EW = ew;
    }

    public Coordinates(int landcell, double yOffset, double xOffset)
    {
        NS = LandblockToNS(landcell, yOffset);
        EW = LandblockToEW(landcell, xOffset);
    }

    public Coordinates(int landcell, double yOffset, double xOffset, int precision)
    {
        NS = Math.Round(LandblockToNS(landcell, yOffset), precision);
        EW = Math.Round(LandblockToEW(landcell, xOffset), precision);
    }

    public static double LandblockToNS(int landcell, double yOffset)
    {
        uint l = (uint)((landcell & 0x00FF0000) / 0x2000);
        return (l + yOffset / 24.0 - 1019.5) / 10.0;
    }

    public static double LandblockToEW(int landcell, double xOffset)
    {
        uint l = (uint)((landcell & 0xFF000000) / 0x200000);
        return (l + xOffset / 24.0 - 1019.5) / 10.0;
    }

    public double AngleTo(Coordinates dest)
    {
        return Math.Atan2(dest.EW - EW, dest.NS - NS);
    }

    public double DistanceTo(Coordinates dest)
    {
        double x = dest.EW - EW;
        double y = dest.NS - NS;
        if (double.IsNaN(x + y))
            return double.PositiveInfinity;
        return Math.Sqrt(x * x + y * y);
    }

    public Coordinates RelativeTo(Coordinates dest)
    {
        return new Coordinates(dest.NS - NS, dest.EW - EW);
    }

    public static Coordinates Round(Coordinates coords, int precision)
    {
        return new Coordinates(Math.Round(coords.NS, precision), Math.Round(coords.EW, precision));
    }

    public static bool TryParse(string? parseString, out Coordinates coords)
    {
        if (parseString == null)
        {
            coords = NoCoordinates;
            return false;
        }
        return FromRegexMatch(FindCoords(parseString), out coords);
    }

    public static bool TryParse(string? parseString, bool allowNoCoords, out Coordinates coords)
    {
        if (
            allowNoCoords
            && (
                parseString == ""
                || parseString == NoCoordinatesString
                || parseString == UnknownCoordinatesString
            )
        )
        {
            coords = NoCoordinates;
            return true;
        }
        return TryParse(parseString, out coords);
    }

    public static bool FromRegexMatch(Match m, out Coordinates coords)
    {
        coords = NoCoordinates;
        if (!m.Success)
            return false;

        double ns = double.Parse(m.Groups["NSval"].Value, CultureInfo.InvariantCulture);
        if (m.Groups["NSchr"].Value.ToLowerInvariant() == "s")
            ns = -ns;

        double ew = double.Parse(m.Groups["EWval"].Value, CultureInfo.InvariantCulture);
        if (m.Groups["EWchr"].Value.ToLowerInvariant() == "w")
            ew = -ew;

        coords = new Coordinates(ns, ew);
        return true;
    }

    private const string RegExDouble = @"(\d{1,3}(\.\d{1,4})?)|(\.\d{1,4})";

    private static readonly Regex CoordSearchRegex = new(
        @"(?<NSval>"
            + RegExDouble
            + @")\s*(?<NSchr>[ns])"
            + @"[;/,\s]{0,4}\s*"
            + @"(?<EWval>"
            + RegExDouble
            + @")\s*(?<EWchr>[ew])",
        RegexOptions.Compiled | RegexOptions.IgnoreCase | RegexOptions.CultureInvariant
    );

    public static MatchCollection FindAllCoords(string parseString)
    {
        return CoordSearchRegex.Matches(parseString);
    }

    public static Match FindCoords(string parseString)
    {
        return CoordSearchRegex.Match(parseString);
    }

    public override string ToString() => ToString(false);

    public string ToString(bool useUnknownString)
    {
        if (double.IsNaN(NS) || double.IsNaN(EW))
            return useUnknownString ? UnknownCoordinatesString : NoCoordinatesString;

        double ns10 = NS * 10;
        double ew10 = EW * 10;
        if (Math.Floor(ns10) != ns10 || Math.Floor(ew10) != ew10)
            return ToString("0.00", useUnknownString);
        else
            return ToString("0.0", useUnknownString);
    }

    public string ToString(string numberFormat)
    {
        return ToString(numberFormat, false);
    }

    public string ToString(string numberFormat, bool useUnknownString)
    {
        if (double.IsNaN(NS) || double.IsNaN(EW))
            return useUnknownString ? UnknownCoordinatesString : NoCoordinatesString;

        return Math.Abs(NS).ToString(numberFormat, CultureInfo.InvariantCulture)
            + (NS >= 0 ? "N" : "S")
            + ", "
            + Math.Abs(EW).ToString(numberFormat, CultureInfo.InvariantCulture)
            + (EW >= 0 ? "E" : "W");
    }

    public static bool operator ==(Coordinates a, Coordinates b)
    {
        return (a.NS == b.NS && a.EW == b.EW) || (double.IsNaN(a.NS) && double.IsNaN(b.NS));
    }

    public static bool operator !=(Coordinates a, Coordinates b)
    {
        return !(a == b);
    }

    public override bool Equals(object? obj)
    {
        return obj is Coordinates c && this == c;
    }

    public bool Equals(Coordinates other) => this == other;

    public override int GetHashCode()
    {
        if (double.IsNaN(NS) || double.IsNaN(EW))
            return int.MaxValue;
        return (int)(NS * 1000000) ^ (int)(EW * 10000);
    }
}
