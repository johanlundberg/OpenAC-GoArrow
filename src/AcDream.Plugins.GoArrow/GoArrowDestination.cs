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
    Recall
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

    /// <summary>The current destination location, if set.</summary>
    public RouteFinding.Location? TargetLocation { get; private set; }

    /// <summary>The semantic kind of the current destination.</summary>
    public GoArrowDestinationKind Kind { get; private set; } = GoArrowDestinationKind.Location;

    /// <summary>Original coordinate text used to create a coordinate destination.</summary>
    public string CoordinateText { get; private set; } = string.Empty;

    /// <summary>Selected object id when this is an object destination.</summary>
    public uint? TargetObjectId { get; private set; }

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

    /// <summary>Bearing to the destination in degrees, or NaN.</summary>
    public double BearingDegrees { get; internal set; } = double.NaN;

    public GoArrowDestination(GoArrowSettings settings, LocationDatabase database, RouteFinder routeFinder)
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
        TargetUnavailable = false;
        _settings.DestinationName = loc.Name;
        CurrentRoute = null;
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
        TargetUnavailable = false;
        _settings.DestinationName = location.Name;
        CurrentRoute = null;
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
        TargetUnavailable = false;
        _settings.DestinationName = CoordinateText;
        CurrentRoute = null;
    }

    /// <summary>Sets a destination from a selected world object snapshot.</summary>
    public bool SetObject(PluginWorldObject obj)
    {
        if (obj.ObjectId == 0 || !obj.HasPosition)
            return false;
        TargetLocation = new RouteFinding.Location(
            string.IsNullOrWhiteSpace(obj.Name) ? $"Object 0x{obj.ObjectId:X8}" : obj.Name,
            obj.Position.NorthSouth,
            obj.Position.EastWest);
        Kind = GoArrowDestinationKind.Object;
        TargetObjectId = obj.ObjectId;
        TargetUnavailable = false;
        CoordinateText = string.Empty;
        _settings.DestinationName = TargetLocation.Name;
        CurrentRoute = null;
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
            obj.Position.EastWest);
        TargetUnavailable = false;
        CurrentRoute = null;
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
        TargetUnavailable = false;
        _settings.DestinationName = string.Empty;
        CurrentRoute = null;
        EstimatedDistance = double.NaN;
        BearingDegrees = double.NaN;
    }

    /// <summary>
    /// Calculate or recalculate the route from the current position.
    /// </summary>
    public void CalculateRoute(RouteFinding.Location currentPosition)
    {
        if (TargetLocation == null)
        {
            CurrentRoute = null;
            return;
        }

        CurrentRoute = _routeFinder.FindRoute(currentPosition, TargetLocation, _settings.RouteCostProfile);

        EstimatedDistance = currentPosition.DistanceTo(TargetLocation);
        BearingDegrees = currentPosition.AngleTo(TargetLocation) * (180.0 / Math.PI);
    }

    /// <summary>
    /// Find the next immediate sub-destination (the first step's destination).
    /// </summary>
    public RouteFinding.Location? GetImmediateTarget()
    {
        if (CurrentRoute == null || CurrentRoute.StepCount == 0)
            return TargetLocation;

        return CurrentRoute.Steps[0].To;
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

    public bool MoveRouteStep(int fromIndex, int toIndex) => CurrentRoute?.MoveStep(fromIndex, toIndex) == true;

    /// <summary>
    /// Get all known destination names.
    /// </summary>
    public List<string> GetAllDestinationNames()
    {
        return _routeFinder.GetAllDestinationNames();
    }
}