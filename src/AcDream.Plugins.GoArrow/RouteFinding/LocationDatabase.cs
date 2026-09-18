using System.Globalization;
using System.Xml;

namespace AcDream.Plugins.GoArrow.RouteFinding;

/// <summary>
/// Database of named locations, loaded from XML embedded resources.
/// Ported from GoArrow LocationDatabase.cs — Decal dependencies removed.
/// Uses simple XML parsing instead of Decal XML services.
/// </summary>
public class LocationDatabase
{
    private readonly List<Location> _locations = new();
    private readonly Dictionary<string, List<Location>> _locationsByName = new(StringComparer.OrdinalIgnoreCase);
    private readonly List<PortalDevice> _portalDevices = new();
    private readonly List<RouteStart> _routeStarts = new();
    private readonly object _lock = new();

    /// <summary>
    /// All locations currently loaded.
    /// </summary>
    public IReadOnlyList<Location> AllLocations
    {
        get { lock (_lock) return _locations.ToArray(); }
    }

    /// <summary>
    /// All portal device records.
    /// </summary>
    public IReadOnlyList<PortalDevice> PortalDevices
    {
        get { lock (_lock) return _portalDevices.ToArray(); }
    }

    /// <summary>
    /// All route start records.
    /// </summary>
    public IReadOnlyList<RouteStart> RouteStarts
    {
        get { lock (_lock) return _routeStarts.ToArray(); }
    }

    /// <summary>
    /// Number of locations in the database.
    /// </summary>
    public int LocationCount
    {
        get { lock (_lock) return _locations.Count; }
    }

    /// <summary>
    /// Load locations from an XML string.
    /// Expected format: &lt;Locations&gt;&lt;Location name="..."&gt;&lt;Coords NS="..." EW="..."/&gt;&lt;/Location&gt;...&lt;/Locations&gt;
    /// </summary>
    public void LoadLocationsXml(string xml)
    {
        var doc = new XmlDocument();
        doc.LoadXml(xml);

        var newLocations = new List<Location>();
        bool isWarcryAtlas = string.Equals(
            doc.DocumentElement?.Name,
            "atlas",
            StringComparison.OrdinalIgnoreCase);

        string xpath = isWarcryAtlas ? "//location" : "//Location|//loc";
        var locationNodes = doc.SelectNodes(xpath);
        if (locationNodes != null)
        {
            foreach (XmlNode node in locationNodes)
            {
                if (node is not XmlElement element)
                    continue;

                Location location = isWarcryAtlas
                    ? Location.FromXmlWarcry(element)
                    : Location.FromXml(element);
                if (location.HasCoordinates)
                    newLocations.Add(location);
            }
        }

        // Fallback: CSV-style text nodes.
        if (newLocations.Count == 0)
        {
            var textNodes = doc.SelectNodes("//Location/text()|//loc/text()");
            if (textNodes != null)
            {
                foreach (XmlNode node in textNodes)
                {
                    var location = Location.FromCsvLine(node.Value ?? string.Empty);
                    if (location.HasCoordinates)
                        newLocations.Add(location);
                }
            }
        }

        lock (_lock)
        {
            _locations.Clear();
            _locationsByName.Clear();
            foreach (var loc in newLocations)
            {
                _locations.Add(loc);
                if (!_locationsByName.ContainsKey(loc.Name))
                    _locationsByName[loc.Name] = new List<Location>();
                _locationsByName[loc.Name].Add(loc);
            }
        }
    }

    /// <summary>
    /// Load portal device records from an XML string.
    /// </summary>
    public void LoadPortalDevicesXml(string xml)
    {
        var doc = new XmlDocument();
        doc.LoadXml(xml);

        var newDevices = new List<PortalDevice>();

        var deviceNodes = doc.SelectNodes("//Device");
        if (deviceNodes != null)
        {
            foreach (XmlNode node in deviceNodes)
            {
                var dest = node.Attributes?["Destination"]?.Value ?? string.Empty;
                var via = node.Attributes?["Via"]?.Value ?? string.Empty;
                var landmass = node.Attributes?["Landmass"]?.Value ?? string.Empty;
                newDevices.Add(new PortalDevice(dest, via, landmass));
            }
        }

        if (newDevices.Count == 0)
        {
            var textNodes = doc.SelectNodes("//Device/text()");
            if (textNodes != null)
            {
                foreach (XmlNode node in textNodes)
                {
                    newDevices.Add(PortalDevice.FromCsvLine(node.Value ?? string.Empty));
                }
            }
        }

        lock (_lock)
        {
            _portalDevices.Clear();
            _portalDevices.AddRange(newDevices);
        }
    }

    /// <summary>
    /// Load route start records from an XML string.
    /// </summary>
    public void LoadRouteStartsXml(string xml)
    {
        var doc = new XmlDocument();
        doc.LoadXml(xml);

        var newStarts = new List<RouteStart>();

        var startNodes = doc.SelectNodes("//Start");
        if (startNodes != null)
        {
            foreach (XmlNode node in startNodes)
            {
                var dest = node.Attributes?["Destination"]?.Value ?? string.Empty;
                var from = node.Attributes?["From"]?.Value ?? string.Empty;
                var via = node.Attributes?["Via"]?.Value ?? string.Empty;
                newStarts.Add(new RouteStart(dest, from, via));
            }
        }

        if (newStarts.Count == 0)
        {
            var textNodes = doc.SelectNodes("//Start/text()");
            if (textNodes != null)
            {
                foreach (XmlNode node in textNodes)
                {
                    newStarts.Add(RouteStart.FromCsvLine(node.Value ?? string.Empty));
                }
            }
        }

        lock (_lock)
        {
            _routeStarts.Clear();
            _routeStarts.AddRange(newStarts);
        }
    }

    /// <summary>
    /// Load locations from CSV lines.
    /// </summary>
    public void LoadLocationsCsv(IEnumerable<string> lines)
    {
        var newLocations = new List<Location>();
        foreach (var line in lines)
        {
            var trimmed = line.Trim();
            if (trimmed == string.Empty || trimmed.StartsWith('#'))
                continue;
            var loc = Location.FromCsvLine(trimmed);
            if (loc.HasCoordinates)
                newLocations.Add(loc);
        }

        lock (_lock)
        {
            _locations.Clear();
            _locationsByName.Clear();
            foreach (var loc in newLocations)
            {
                _locations.Add(loc);
                if (!_locationsByName.ContainsKey(loc.Name))
                    _locationsByName[loc.Name] = new List<Location>();
                _locationsByName[loc.Name].Add(loc);
            }
        }
    }

    /// <summary>
    /// Load portal devices from CSV lines.
    /// </summary>
    public void LoadPortalDevicesCsv(IEnumerable<string> lines)
    {
        var newDevices = new List<PortalDevice>();
        foreach (var line in lines)
        {
            var trimmed = line.Trim();
            if (trimmed == string.Empty || trimmed.StartsWith('#'))
                continue;
            newDevices.Add(PortalDevice.FromCsvLine(trimmed));
        }

        lock (_lock)
        {
            _portalDevices.Clear();
            _portalDevices.AddRange(newDevices);
        }
    }

    /// <summary>
    /// Load route starts from CSV lines.
    /// </summary>
    public void LoadRouteStartsCsv(IEnumerable<string> lines)
    {
        var newStarts = new List<RouteStart>();
        foreach (var line in lines)
        {
            var trimmed = line.Trim();
            if (trimmed == string.Empty || trimmed.StartsWith('#'))
                continue;
            newStarts.Add(RouteStart.FromCsvLine(trimmed));
        }

        lock (_lock)
        {
            _routeStarts.Clear();
            _routeStarts.AddRange(newStarts);
        }
    }

    /// <summary>
    /// Find a location by name (first match, case-insensitive).
    /// </summary>
    public Location? FindLocation(string name)
    {
        lock (_lock)
        {
            if (_locationsByName.TryGetValue(name, out var matches) && matches.Count > 0)
                return matches[0];
            return null;
        }
    }

    ///<summary>
    /// Find all locations matching a name (case-insensitive).
    ///</summary>
    public IReadOnlyList<Location> FindLocations(string name)
    {
        lock (_lock)
        {
            if (_locationsByName.TryGetValue(name, out var matches))
               return matches.ToArray();
            return Array.Empty<Location>();
        }
    }

    /// <summary>
    /// Find locations whose name contains the given substring (case-insensitie).
    /// </summary>
    public List<Location> SearchLocations(string substring)
    {
        lock (_lock)
        {
            return _locations.Where(l => l.Name.Contains(substring, StringComparison.OrdinalIgnoreCase)).ToList();
        }
    }

    /// <summary>
    /// Find all portal devices that can reach a destination.
    /// </summary>
    public List<PortalDevice> FindPortalsTo(string destination)
    {
        lock (_lock)
        {
            return _portalDevices.Where(p =>
                string.Equals(p.Destination, destination, StringComparison.OrdinalIgnoreCase)).ToList();
        }
    }

    /// <summary>
    /// Find all route starts from a given origin.
    /// </summary>
    public List<RouteStart> FindRoutesFrom(string origin)
    {
        lock (_lock)
        {
            return _routeStarts.Where(r =>
                string.Equals(r.From, origin, StringComparison.OrdinalIgnoreCase)).ToList();
        }
    }

    /// <summary>
    /// Find all route starts going to a destination.
    /// </summary>
    public List<RouteStart> FindRoutesTo(string destination)
    {
        lock (_lock)
        {
            return _routeStarts.Where(r =>
                string.Equals(r.Destination, destination, StringComparison.OrdinalIgnoreCase)).ToList();
        }
    }

    /// <summary>
    /// Clear all data.
    /// </summary>
    public void Clear()
    {
        lock (_lock)
        {
            _locations.Clear();
            _locationsByName.Clear();
            _portalDevices.Clear();
            _routeStarts.Clear();
        }
    }
}