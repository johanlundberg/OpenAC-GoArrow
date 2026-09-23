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

    /// <summary>Keep the exact route origin when it shares a graph node's coordinates.</summary>
    public void SetFirstStepOrigin(Location origin)
    {
        if (_steps.Count == 0)
            return;
        RouteStep first = _steps[0];
        _steps[0] = new RouteStep(first.Kind, origin, first.To, first.Distance, first.Via)
        {
            ObjectId = first.ObjectId,
            ObjectCapabilities = first.ObjectCapabilities,
            InteractionTimeout = first.InteractionTimeout,
            MaxRetries = first.MaxRetries,
        };
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

    /// <summary>Removes one route step and rebuilds aggregate counters.</summary>
    public bool RemoveStep(int index)
    {
        if (index < 0 || index >= _steps.Count) return false;
        _steps.RemoveAt(index);
        RecalculateTotals();
        return true;
    }

    /// <summary>Moves a route step while preserving deterministic ordering.</summary>
    public bool MoveStep(int fromIndex, int toIndex)
    {
        if (fromIndex < 0 || fromIndex >= _steps.Count || toIndex < 0 || toIndex >= _steps.Count) return false;
        RouteStep step = _steps[fromIndex];
        _steps.RemoveAt(fromIndex);
        _steps.Insert(toIndex, step);
        return true;
    }

    private void RecalculateTotals()
    {
        TotalDistance = _steps.Where(step => step.Kind == RouteStepKind.Travel).Sum(step => step.Distance);
        PortalCount = _steps.Count(step => step.Kind == RouteStepKind.Portal);
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
        return $"{Destination}: {StepCount} steps, {TravelDistance.Format(TotalDistance)}, {PortalCount} portals";
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
            RouteStepKind.Travel when Via.Equals("Arrived", StringComparison.OrdinalIgnoreCase)
                => $"Arrived: {To.Name}",
            RouteStepKind.Travel => $"Walk: {To.Name} ({TravelDistance.Format(Distance)})",
            RouteStepKind.Portal => $"Portal: {(string.IsNullOrWhiteSpace(Via) ? To.Name : Via)}",
            RouteStepKind.Recall => $"Recall: {(string.IsNullOrWhiteSpace(Via) ? To.Name : Via)}",
            _ => To.Name
        };
    }
}
