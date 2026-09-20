using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow;

/// <summary>
/// Data-binding surface for the GoArrow plugin's declarative panel.
/// Properties and actions are consumed by goarrow-panel.xml markup.
/// </summary>
internal sealed class GoArrowPanel
{
    private readonly IPluginHost _host;
    private readonly GoArrowPlugin _plugin;
    private readonly GoArrowSettings _settings;
    private readonly GoArrowDestination _destination;
    private readonly GoArrowNavigator _navigator;

    public GoArrowPanel(
        IPluginHost host,
        GoArrowPlugin plugin,
        GoArrowSettings settings,
        GoArrowDestination destination,
        GoArrowNavigator navigator)
    {
        _host = host;
        _plugin = plugin;
        _settings = settings;
        _destination = destination;
        _navigator = navigator;
    }

    // ── Panel display properties ────────────────────────────────────

    /// <summary>The current destination name.</summary>
    public string DestinationText =>
        string.IsNullOrEmpty(_destination.TargetName)
            ? "[None]"
            : _destination.TargetName;

    /// <summary>Distance to destination (formatted).</summary>
    public string DistanceText =>
        _destination.HasDestination && _settings.ShowDistance
            ? $"{_destination.EstimatedDistance:F2} mu"
            : string.Empty;

    /// <summary>Bearing to destination (formatted).</summary>
    public string BearingText =>
        _destination.HasDestination && _settings.ShowBearing
            ? $"{_destination.BearingDegrees:F1}°"
            : string.Empty;

    /// <summary>Navigation status text.</summary>
    public string NavStatusText
    {
        get
        {
            if (_navigator.IsNavigating)
                return "Navigating...";
            if (_navigator.WaitingForInteraction)
                return "Waiting for interaction";
            if (_navigator.HasArrived)
                return "Arrived!";
            return string.IsNullOrEmpty(_destination.TargetName) ? "Idle" : "Ready";
        }
    }

    /// <summary>Route step count.</summary>
    public string RouteStepsText
    {
        get
        {
            var route = _destination.CurrentRoute;
            if (route == null)
                return "0 steps";
            return $"{route.StepCount} steps";
        }
    }

    /// <summary>The currently active route leg.</summary>
    public string CurrentLegText =>
        _destination.CurrentRoute?.Steps.FirstOrDefault()?.ToString() ?? "No active route leg";

    /// <summary>The latest host navigation report.</summary>
    public string NavigationReportText => _navigator.LastReport;

    /// <summary>Whether a destination is set (enables start/stop buttons).</summary>
    public bool HasDestination => _destination.HasDestination;

    /// <summary>Whether navigation is active.</summary>
    public bool IsNavigating => _navigator.IsNavigating;

    /// <summary>Whether the route is paused for a manual portal/recall action.</summary>
    public bool WaitingForInteraction => _navigator.WaitingForInteraction;

    /// <summary>Editable destination input used by panel hosts that support text controls.</summary>
    public string DestinationInput { get; set; } = string.Empty;

    public IReadOnlyList<string> DestinationSuggestions => string.IsNullOrWhiteSpace(DestinationInput)
        ? Array.Empty<string>()
        : _plugin.SearchLocations(DestinationInput).Take(12).Select(location => location.Name).ToArray();

    public IReadOnlyList<string> RouteSteps => _plugin.GetCurrentRouteSteps();

    public string FailureDiagnostics => string.IsNullOrEmpty(_navigator.FailureReason)
        ? _navigator.LastReport
        : _navigator.FailureReason;

    public string ProgressText => _destination.CurrentRoute is { } route
        ? $"Leg {_navigator.LegIndex + 1}/{Math.Max(1, route.StepCount)}"
        : "No route";

    public void SubmitDestination()
    {
        if (string.IsNullOrWhiteSpace(DestinationInput))
            return;
        if (!_plugin.TrySetCoordinateDestination(DestinationInput))
            _plugin.SetDestination(DestinationInput.Trim());
    }

    public void SelectSuggestion(string name)
    {
        DestinationInput = name;
        SubmitDestination();
    }

    // ── Toggle settings ─────────────────────────────────────────────

    public bool AutoNavigate
    {
        get => _settings.AutoNavigate;
        set
        {
            _settings.AutoNavigate = value;
            _settings.Save(_host.Storage);
        }
    }

    public bool ShowDistance
    {
        get => _settings.ShowDistance;
        set
        {
            _settings.ShowDistance = value;
            _settings.Save(_host.Storage);
        }
    }

    public bool ShowBearing
    {
        get => _settings.ShowBearing;
        set
        {
            _settings.ShowBearing = value;
            _settings.Save(_host.Storage);
        }
    }

    public bool RecalculateRoute
    {
        get => _settings.RecalculateRoute;
        set
        {
            _settings.RecalculateRoute = value;
            _settings.Save(_host.Storage);
        }
    }

    // ── Actions bound to the panel ──────────────────────────────────

    /// <summary>Toggle auto-navigate on/off.</summary>
    public Action ToggleAutoNavigate => () =>
    {
        AutoNavigate = !AutoNavigate;
        _host.Log.Info($"GoArrow: Auto-navigate = {AutoNavigate}");
    };

    /// <summary>Toggle distance display.</summary>
    public Action ToggleShowDistance => () => ShowDistance = !ShowDistance;

    /// <summary>Toggle bearing display.</summary>
    public Action ToggleShowBearing => () => ShowBearing = !ShowBearing;

    /// <summary>Start navigation.</summary>
    public Action StartNavigation => () => _navigator.StartNavigation();

    /// <summary>Stop navigation.</summary>
    public Action StopNavigation => () => _plugin.StopNavigation();

    /// <summary>Resume after manually completing a portal or recall action.</summary>
    public Action ResumeNavigation => () => _navigator.ResumeAfterInteraction();

    /// <summary>Clear destination.</summary>
    public Action ClearDestination => () => _plugin.ClearDestination();

    /// <summary>Show destination input hint.</summary>
    public Action ShowDestinationInput => () =>
        _host.Automation.Chat.PostSystemMessage("GoArrow: Use /go <destination> in chat to set a destination.");

    // ── Tick update ─────────────────────────────────────────────────

    public void OnTick(double elapsed)
    {
        // Properties are read by the panel binding; no explicit update needed.
    }
}