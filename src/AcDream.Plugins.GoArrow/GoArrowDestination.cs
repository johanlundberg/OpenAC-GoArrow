using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow;

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
        _settings.DestinationName = location.Name;
        CurrentRoute = null;
    }

    /// <summary>
    /// Clear the current destination.
    /// </summary>
    public void ClearDestination()
    {
        TargetLocation = null;
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

        CurrentRoute = _routeFinder.FindRoute(currentPosition, TargetLocation);

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

    /// <summary>
    /// Get all known destination names.
    /// </summary>
    public List<string> GetAllDestinationNames()
    {
        return _routeFinder.GetAllDestinationNames();
    }
}