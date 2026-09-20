using System.Globalization;
using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow.RouteFinding;

/// <summary>
/// A route is a list of <see cref="RouteStep"/> objects connecting an origin to a destination.
/// Ported from GoArrow Route.cs — Decal dependencies removed.
/// </summary>
public class Route
{
    private readonly List<RouteStep> _steps = new();

    /// <summary>The destination name this route leads to.</summary>
    public string Destination { get; set; } = string.Empty;

    /// <summary>The human-readable description of this route.</summary>
    public string Description { get; set; } = string.Empty;

    /// <summary>The ordered list of route steps.</summary>
    public IReadOnlyList<RouteStep> Steps => _steps.AsReadOnly();

    /// <summary>The total number of steps in this route.</summary>
    public int StepCount => _steps.Count;

    /// <summary>The total distance of all travel steps in map units.</summary>
    public double TotalDistance { get; private set; }

    /// <summary>The number of portal steps.</summary>
    public int PortalCount { get; private set; }

    public Route() { }

    public Route(string destination)
    {
        Destination = destination;
    }

    public void AddStep(RouteStep step)
    {
        _steps.Add(step);
        if (step.Kind == RouteStepKind.Travel)
            TotalDistance += step.Distance;
        if (step.Kind == RouteStepKind.Portal)
            PortalCount++;
    }

    /// <summary>
    /// Insert a step at the beginning of the route.
    /// </summary>
    public void PrependStep(RouteStep step)
    {
        _steps.Insert(0, step);
        if (step.Kind == RouteStepKind.Travel)
            TotalDistance += step.Distance;
        if (step.Kind == RouteStepKind.Portal)
            PortalCount++;
    }

    public void AddTravelStep(Location from, Location to, string via = "")
    {
        double distance = from.Coords.DistanceTo(to.Coords);
        AddStep(new RouteStep(RouteStepKind.Travel, from, to, distance, via));
    }

    public void AddPortalStep(Location from, Location to, string portalName)
    {
        AddStep(new RouteStep(RouteStepKind.Portal, from, to, 0, portalName));
    }

    public void AddRecallStep(Location from, Location to, string recallName)
    {
        AddStep(new RouteStep(RouteStepKind.Recall, from, to, 0, recallName));
    }

    /// <summary>
    /// Clears all steps.
    /// </summary>
    public void Clear()
    {
        _steps.Clear();
        TotalDistance = 0;
        PortalCount = 0;
    }

    public override string ToString()
    {
        return $"{Destination}: {StepCount} steps, {TotalDistance:F2}mu, {PortalCount} portals";
    }
}

public enum RouteStepKind
{
    Travel,
    Portal,
    Recall
}

public class RouteStep
{
    public RouteStepKind Kind { get; }
    public Location From { get; }
    public Location To { get; }
    public double Distance { get; }
    public string Via { get; }

    /// <summary>Semantic object/action metadata for interaction-driven legs.</summary>
    public uint ObjectId { get; init; }
    public PluginObjectCapabilities ObjectCapabilities { get; init; }
    public TimeSpan InteractionTimeout { get; init; } = TimeSpan.FromSeconds(15);
    public int MaxRetries { get; init; } = 1;

    public RouteStep(RouteStepKind kind, Location from, Location to, double distance, string via)
    {
        Kind = kind;
        From = from;
        To = to;
        Distance = distance;
        Via = via ?? string.Empty;
    }

    public override string ToString()
    {
        return Kind switch
        {
            RouteStepKind.Travel => $"{Via}: {From.Name} → {To.Name} ({Distance:F2}mu)",
            RouteStepKind.Portal => $"Portal [{Via}]: {From.Name} → {To.Name}",
            RouteStepKind.Recall => $"Recall [{Via}]: {From.Name} → {To.Name}",
            _ => $"{From.Name} → {To.Name}"
        };
    }
}