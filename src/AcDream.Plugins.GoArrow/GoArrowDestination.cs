using AcDream.Plugin.Abstractions;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow;

/// <summary>Identifies the semantic source of a GoArrow destination.</summary>
internal enum GoArrowDestinationKind
{
    Location,
    Coordinates,
    Object,
    Route,
    Recall,
}

/// <summary>
/// Destination tracking and navigation state for GoArrow.
/// Wraps the route-finding logic and current destination.
/// </summary>
internal sealed class GoArrowDestination
{
    private readonly GoArrowSettings _settings;
    private readonly RouteFinder _routeFinder;
    private readonly LocationDatabase _database;
    private IReadOnlyList<Route> _alternatives = [];
    private RouteFinding.Location? _routeOrigin;
    private int _alternativeIndex;
    private bool _alternativesExhausted;

    /// <summary>The current destination location, if set.</summary>
    public RouteFinding.Location? TargetLocation { get; private set; }

    /// <summary>The semantic kind of the current destination.</summary>
    public GoArrowDestinationKind Kind { get; private set; } = GoArrowDestinationKind.Location;

    /// <summary>Original coordinate text used to create a coordinate destination.</summary>
    public string CoordinateText { get; private set; } = string.Empty;

    /// <summary>Selected object id when this is an object destination.</summary>
    public uint? TargetObjectId { get; private set; }

    /// <summary>The last live cell and floor position of an object destination.</summary>
    public PluginNavigationPosition? TargetObjectPosition { get; private set; }

    /// <summary>The exact cell and floor of an indoor point destination.</summary>
    public PluginNavigationPosition? TargetIndoorPosition { get; private set; }

    /// <summary>Whether an object destination no longer has a live position.</summary>
    public bool TargetUnavailable { get; private set; }

    /// <summary>The current computed route, if any.</summary>
    public Route? CurrentRoute { get; private set; }

    /// <summary>Whether a destination is currently set.</summary>
    public bool HasDestination => TargetLocation != null;

    /// <summary>The name of the current destination, or empty.</summary>
    public string TargetName => TargetLocation?.Name ?? string.Empty;

    /// <summary>Estimated distance to the destination, or NaN.</summary>
    public double EstimatedDistance { get; internal set; } = double.NaN;

    /// <summary>Bearing to the next route waypoint in degrees, or NaN.</summary>
    public double BearingDegrees { get; internal set; } = double.NaN;

    /// <summary>Distance to the next route waypoint, or NaN.</summary>
    public double GuidanceDistance { get; private set; } = double.NaN;

    public GoArrowDestination(
        GoArrowSettings settings,
        LocationDatabase database,
        RouteFinder routeFinder
    )
    {
        _settings = settings;
        _database = database;
        _routeFinder = routeFinder;
    }

    /// <summary>
    /// Set a destination by name.
    /// </summary>
    /// <returns>True if the destination was found in the database.</returns>
    public bool SetDestination(string name)
    {
        var loc = _database.FindLocation(name);
        if (loc == null)
            return false;

        TargetLocation = loc;
        Kind = GoArrowDestinationKind.Location;
        CoordinateText = string.Empty;
        TargetObjectId = null;
        TargetObjectPosition = null;
        TargetIndoorPosition = loc.IndoorPosition;
        TargetUnavailable = false;
        _settings.DestinationName = loc.Name;
        CurrentRoute = null;
        ResetAlternatives();
        return true;
    }

    /// <summary>
    /// Set a destination directly from a Location object.
    /// </summary>
    public void SetDestination(RouteFinding.Location location)
    {
        TargetLocation = location;
        Kind = GoArrowDestinationKind.Location;
        CoordinateText = string.Empty;
        TargetObjectId = null;
        TargetObjectPosition = null;
        TargetIndoorPosition = location.IndoorPosition;
        TargetUnavailable = false;
        _settings.DestinationName = location.Name;
        CurrentRoute = null;
        ResetAlternatives();
    }

    /// <summary>Sets a destination at an arbitrary coordinate.</summary>
    public void SetCoordinate(double northSouth, double eastWest, string? displayText = null)
    {
        CoordinateText = string.IsNullOrWhiteSpace(displayText)
            ? $"{northSouth:0.###}N {eastWest:0.###}E"
            : displayText.Trim();
        TargetLocation = new RouteFinding.Location(CoordinateText, northSouth, eastWest);
        Kind = GoArrowDestinationKind.Coordinates;
        TargetObjectId = null;
        TargetObjectPosition = null;
        TargetIndoorPosition = null;
        TargetUnavailable = false;
        _settings.DestinationName = CoordinateText;
        CurrentRoute = null;
        ResetAlternatives();
    }

    /// <summary>Sets a destination from a selected world object snapshot.</summary>
    public bool SetObject(PluginWorldObject obj)
    {
        if (obj.ObjectId == 0 || !obj.HasPosition)
            return false;
        TargetLocation = new RouteFinding.Location(
            string.IsNullOrWhiteSpace(obj.Name) ? $"Object 0x{obj.ObjectId:X8}" : obj.Name,
            obj.Position.NorthSouth,
            obj.Position.EastWest
        );
        Kind = GoArrowDestinationKind.Object;
        TargetObjectId = obj.ObjectId;
        TargetObjectPosition = obj.Position;
        TargetIndoorPosition = null;
        TargetUnavailable = false;
        CoordinateText = string.Empty;
        _settings.DestinationName = TargetLocation.Name;
        CurrentRoute = null;
        ResetAlternatives();
        return true;
    }

    /// <summary>Marks an object target unavailable when its snapshot disappears.</summary>
    public void MarkObjectUnavailable()
    {
        if (Kind == GoArrowDestinationKind.Object)
            TargetUnavailable = true;
    }

    /// <summary>Refreshes an object destination from a newer host snapshot.</summary>
    public void UpdateObject(PluginWorldObject obj)
    {
        if (Kind != GoArrowDestinationKind.Object || TargetObjectId != obj.ObjectId)
            return;
        if (!obj.HasPosition)
        {
            MarkObjectUnavailable();
            return;
        }
        TargetLocation = new RouteFinding.Location(
            string.IsNullOrWhiteSpace(obj.Name) ? TargetLocation?.Name ?? "Object" : obj.Name,
            obj.Position.NorthSouth,
            obj.Position.EastWest
        );
        TargetObjectPosition = obj.Position;
        TargetUnavailable = false;
        CurrentRoute = null;
        ResetAlternatives();
    }

    /// <summary>
    /// Clear the current destination.
    /// </summary>
    public void ClearDestination()
    {
        TargetLocation = null;
        Kind = GoArrowDestinationKind.Location;
        CoordinateText = string.Empty;
        TargetObjectId = null;
        TargetObjectPosition = null;
        TargetIndoorPosition = null;
        TargetUnavailable = false;
        _settings.DestinationName = string.Empty;
        CurrentRoute = null;
        ResetAlternatives();
        EstimatedDistance = double.NaN;
        BearingDegrees = double.NaN;
        GuidanceDistance = double.NaN;
    }

    public void ClearRoute() { CurrentRoute = null; ResetAlternatives(); }

    private void ResetAlternatives()
    {
        _alternatives = [];
        _routeOrigin = null;
        _alternativeIndex = 0;
        _alternativesExhausted = false;
    }

    public int AlternativeIndex => _alternativeIndex;
    public int AlternativeCount => _alternatives.Count;
    public bool AlternativeHasMore => !_alternativesExhausted && _alternatives.Count < 8;

    public bool CanKeepSelectedAlternative(PluginNavigationPosition position) =>
        _alternativeIndex > 0 && CurrentRoute is { StepCount: > 0 }
        && _routeOrigin is not null && position.IsOutdoor
        && new RouteFinding.Coordinates(position.NorthSouth, position.EastWest)
            .DistanceTo(_routeOrigin.Coords) * 240 <= 20;

    public bool CycleAlternative(int direction, RouteFinding.Location origin)
    {
        if (TargetLocation is null || CurrentRoute is null || direction is not (-1 or 1))
            return false;
        if (_routeOrigin is null || origin.DistanceTo(_routeOrigin) * 240 > 20)
        {
            _alternatives = [];
            _routeOrigin = origin;
            _alternativeIndex = 0;
            _alternativesExhausted = false;
        }
        int next = _alternativeIndex + direction;
        if (next >= _alternatives.Count && !_alternativesExhausted && _alternatives.Count < 8)
        {
            int requested = Math.Max(2, _alternatives.Count + 1);
            _alternatives = _routeFinder.FindRouteAlternatives(origin, TargetLocation,
                _settings.RouteCostProfile, requested);
            _alternativesExhausted = _alternatives.Count < requested || requested >= 8;
        }
        if (next < 0 || next >= _alternatives.Count)
            return false;
        _alternativeIndex = next;
        CurrentRoute = _alternatives[next];
        UpdateGuidance(origin);
        return true;
    }

    /// <summary>
    /// Calculate or recalculate the route from the current position.
    /// </summary>
    public bool TryPlanDungeonExit(uint cellId)
    {
        if (TargetLocation is null || TargetIndoorPosition is not null
            || TargetObjectPosition is { IsOutdoor: false })
            return false;
        int dungeonId = (int)(cellId >> 16);
        var locations = _database.AllLocations;
        var dungeonNames = locations.Where(l => l.DungeonId == dungeonId && dungeonId != 0)
            .Select(l => l.Name).Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
        var exits = locations.Where(l => l.UseInRouteFinding && !l.IsRetired && l.HasExitCoords
            && dungeonNames.Any(name => l.Name.StartsWith(name + " to ", StringComparison.OrdinalIgnoreCase)))
            .OrderBy(l => l.Name, StringComparer.OrdinalIgnoreCase).ThenBy(l => l.Id);
        RouteFinding.Location? selected = null;
        double bestCost = double.PositiveInfinity;
        foreach (var exit in exits)
        {
            var onward = _routeFinder.FindRoute(new RouteFinding.Location("Dungeon exit", exit.ExitCoords),
                TargetLocation, _settings.RouteCostProfile);
            if (onward.StepCount == 0 || onward.TotalDistance >= bestCost)
                continue;
            bestCost = onward.TotalDistance;
            selected = exit;
        }
        if (selected is null)
            return false;
        CurrentRoute = new Route(TargetLocation.Name);
        CurrentRoute.AddTravelStep(new RouteFinding.Location(
            $"{selected.Name} arrival ({selected.Id})", selected.ExitCoords), TargetLocation,
            "Walk after dungeon exit");
        return true;
    }

    public void CalculateRoute(RouteFinding.Location currentPosition)
    {
        ResetAlternatives();
        if (TargetLocation == null)
        {
            CurrentRoute = null;
            return;
        }

        CurrentRoute = _routeFinder.FindRoute(
            currentPosition,
            TargetLocation,
            _settings.RouteCostProfile
        );
        _routeOrigin = currentPosition;

        UpdateGuidance(currentPosition);
    }

    /// <summary>
    /// Update distance to the final destination and guidance to the next step.
    /// </summary>
    public void UpdateGuidance(RouteFinding.Location currentPosition)
    {
        if (TargetLocation is null)
        {
            EstimatedDistance = double.NaN;
            BearingDegrees = double.NaN;
            GuidanceDistance = double.NaN;
            return;
        }

        EstimatedDistance = currentPosition.DistanceTo(TargetLocation);
        var next = GetImmediateTarget() ?? TargetLocation;
        GuidanceDistance = currentPosition.DistanceTo(next);
        double bearing = currentPosition.AngleTo(next) * (180.0 / Math.PI);
        BearingDegrees = bearing < 0 ? bearing + 360.0 : bearing;
    }

    /// <summary>Find the next waypoint to walk toward or interact with.</summary>
    public RouteFinding.Location? GetImmediateTarget()
    {
        if (CurrentRoute == null || CurrentRoute.StepCount == 0)
            return TargetLocation;

        var step = CurrentRoute.Steps[0];
        return step.Kind == RouteStepKind.Travel ? step.To : step.From;
    }

    /// <summary>
    /// Advance past the first step once it's completed.
    /// </summary>
    public void AdvanceStep()
    {
        if (CurrentRoute == null || CurrentRoute.StepCount == 0)
            return;

        var completedStep = CurrentRoute.Steps[0];
        var nextTarget = completedStep.To;

        // Rewrite route without the completed step
        var newRoute = new Route(CurrentRoute.Destination);
        for (int i = 1; i < CurrentRoute.Steps.Count; i++)
            newRoute.AddStep(CurrentRoute.Steps[i]);

        CurrentRoute = newRoute;

        // If no more steps, we've arrived
        if (CurrentRoute.Steps.Count == 0)
            EstimatedDistance = 0;
    }

    public bool RemoveRouteStep(int index) => CurrentRoute?.RemoveStep(index) == true;

    public bool MoveRouteStep(int fromIndex, int toIndex) =>
        CurrentRoute?.MoveStep(fromIndex, toIndex) == true;

    /// <summary>
    /// Get all known destination names.
    /// </summary>
    public List<string> GetAllDestinationNames()
    {
        return _routeFinder.GetAllDestinationNames();
    }
}
