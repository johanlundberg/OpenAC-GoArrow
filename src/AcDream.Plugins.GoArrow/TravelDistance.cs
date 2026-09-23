using System.Globalization;

namespace AcDream.Plugins.GoArrow;

/// <summary>Converts Dereth coordinate distances to player-facing metric distances.</summary>
internal static class TravelDistance
{
    private const double MetersPerCoordinateUnit = 240;

    internal static string Format(double coordinateUnits)
    {
        double meters = coordinateUnits * MetersPerCoordinateUnit;
        if (!double.IsFinite(meters))
            return "--";
        if (meters > 1000 + 1e-6)
            return (meters / 1000).ToString("0.##", CultureInfo.InvariantCulture) + " km";
        if (meters > 0 && meters < 10)
            return meters.ToString("0.0", CultureInfo.InvariantCulture) + " m";
        return meters.ToString("0", CultureInfo.InvariantCulture) + " m";
    }
}
