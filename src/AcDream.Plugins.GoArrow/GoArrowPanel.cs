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

    /// <summary>Whether a destination is set (enables start/stop buttons).</summary>
    public bool HasDestination => _destination.HasDestination;

    /// <summary>Whether navigation is active.</summary>
    public bool IsNavigating => _navigator.IsNavigating;

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