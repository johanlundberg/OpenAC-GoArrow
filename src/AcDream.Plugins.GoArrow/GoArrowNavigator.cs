using AcDream.Plugin.Abstractions;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow;

/// <summary>
/// Handles integration with OpenAC's INavigationAutomation for route walking.
/// Degrades gracefully if navigation is not available (headless mode).
/// </summary>
internal sealed class GoArrowNavigator : IDisposable
{
    private readonly IPluginHost _host;
    private readonly GoArrowDestination _destination;
    private readonly GoArrowSettings _settings;
    private bool _isNavigating;
    private PluginNavigationPosition? _currentNavPosition;
    private long _activeSequence;
    private long _lastHandledReportRevision;
    private bool _navigationEventsSubscribed;

    /// <summary>Whether navigation is currently active.</summary>
    public bool IsNavigating => _isNavigating;

    /// <summary>Last navigation status report from GoTo.</summary>
    public string LastReport => _host.Automation.Navigation.GoToReport.State.ToString();

    /// <summary>Whether the character is at the destination.</summary>
    public bool HasArrived { get; private set; }

    public GoArrowNavigator(IPluginHost host, GoArrowDestination destination, GoArrowSettings settings)
    {
        _host = host;
        _destination = destination;
        _settings = settings;
    }

    /// <summary>
    /// Subscribe to report events while the plugin is enabled. Tick polling remains
    /// as a compatibility fallback for hosts that do not emit navigation events.
    /// </summary>
    public void Enable()
    {
        if (_navigationEventsSubscribed)
            return;

        _host.Events.NavigationChanged += OnNavigationChanged;
        _navigationEventsSubscribed = true;
    }

    public void Disable()
    {
        if (!_navigationEventsSubscribed)
            return;

        _host.Events.NavigationChanged -= OnNavigationChanged;
        _navigationEventsSubscribed = false;
    }

    /// <summary>
    /// Update the current position from the plugin navigation snapshot.
    /// </summary>
    public void UpdatePosition(PluginNavigationPosition navPosition)
    {
        _currentNavPosition = navPosition;
    }

    /// <summary>
    /// Start navigating to the current destination.
    /// </summary>
    public void StartNavigation()
    {
        if (_destination.TargetLocation == null || _currentNavPosition == null)
            return;

        if (!_host.Automation.IsAvailable)
        {
            _host.Log.Warn("GoArrow: Navigation automation unavailable (not in world?)");
            return;
        }

        var currentLoc = new RouteFinding.Location(
            "Current Position",
            _currentNavPosition.Value.NorthSouth,
            _currentNavPosition.Value.EastWest);

        _destination.CalculateRoute(currentLoc);
        if (_destination.CurrentRoute == null || _destination.CurrentRoute.StepCount == 0)
        {
            _host.Log.Warn("GoArrow: No route found.");
            return;
        }

        var immediateTarget = _destination.GetImmediateTarget();
        if (immediateTarget == null)
            return;

        _isNavigating = true;
        HasArrived = false;
        _activeSequence = 0;
        _lastHandledReportRevision = 0;

        // Issue the GoTo command using OpenAC navigation coordinates
        // CellId=0 lets OpenAC resolve the position by map coordinates
        var navPos = new PluginNavigationPosition(
            CellId: 0,
            EastWest: immediateTarget.Coords.EW,
            NorthSouth: immediateTarget.Coords.NS,
            Elevation: double.NaN,
            HeadingDegrees: 0f,
            IsOutdoor: true);

        var status = _host.Automation.Navigation.GoTo(navPos, (float)_settings.ArrivalDistance);
        if (status != PluginNavigationCommandStatus.Accepted)
        {
            _isNavigating = false;
            _host.Log.Warn($"GoArrow: Navigation request was {status}.");
            return;
        }

        _activeSequence = _host.Automation.Navigation.GoToReport.Sequence;
        _host.Log.Info($"GoArrow: Navigating to {immediateTarget.Name} at ({navPos.EastWest:F2}, {navPos.NorthSouth:F2})");
    }

    /// <summary>
    /// Stop all navigation.
    /// </summary>
    public void StopNavigation()
    {
        if (_isNavigating && _host.Automation.IsAvailable)
        {
            _host.Automation.Navigation.StopGoTo();
        }

        _isNavigating = false;
        HasArrived = false;
        _activeSequence = 0;
        _lastHandledReportRevision = 0;
    }

    /// <summary>
    /// Called on each Tick to check navigation progress.
    /// </summary>
    public void OnTick(double elapsed)
    {
        if (!_isNavigating || _destination.TargetLocation == null || _currentNavPosition == null)
            return;

        var currentLoc = new RouteFinding.Location(
            "Current Position",
            _currentNavPosition.Value.NorthSouth,
            _currentNavPosition.Value.EastWest);

        // Update distance/bearing
        _destination.EstimatedDistance = currentLoc.DistanceTo(_destination.TargetLocation);
        _destination.BearingDegrees = currentLoc.AngleTo(_destination.TargetLocation) * (180.0 / Math.PI);

        // Poll GoTo report for state changes
        HandleNavigationReport(_host.Automation.Navigation.GoToReport);
    }

    private void OnNavigationChanged(PluginGoToReport report)
    {
        if (_isNavigating)
            HandleNavigationReport(report);
    }

    private void HandleNavigationReport(PluginGoToReport report)
    {
        // Reports from another owner or an earlier GoTo must not advance this route.
        if (_activeSequence != 0 && report.Sequence != 0 && report.Sequence != _activeSequence)
            return;

        if (report.Revision != 0 && report.Revision == _lastHandledReportRevision)
            return;

        if (report.State is PluginGoToState.Arrived or PluginGoToState.ArrivedWithoutSight)
        {
            _lastHandledReportRevision = report.Revision;
            HandleWaypointReached();
            return;
        }

        if (report.State is PluginGoToState.NoRoute
            or PluginGoToState.Blocked
            or PluginGoToState.Interrupted
            or PluginGoToState.Lost)
        {
            _lastHandledReportRevision = report.Revision;
            _isNavigating = false;
            _host.Log.Warn($"GoArrow: Navigation stopped with state {report.State}: {report.Reason ?? "no reason"}.");
            _host.Automation.Chat.PostSystemMessage($"GoArrow: Navigation stopped ({report.State}).");
        }
    }

    private void HandleWaypointReached()
    {
        if (_currentNavPosition == null)
        {
            StopNavigation();
            return;
        }

        // Advance to next step
        _destination.AdvanceStep();
        if (_destination.CurrentRoute == null || _destination.CurrentRoute.StepCount == 0)
        {
            // Arrived at final destination
            HasArrived = true;
            _isNavigating = false;
            _host.Automation.Navigation.StopGoTo();
            _activeSequence = 0;
            _host.Automation.Chat.PostSystemMessage($"GoArrow: Arrived at '{_destination.TargetName}'.");
            return;
        }

        // Continue to next leg
        if (_settings.RecalculateRoute)
        {
            var currentLoc = new RouteFinding.Location(
                "Current Position",
                _currentNavPosition.Value.NorthSouth,
                _currentNavPosition.Value.EastWest);
            _destination.CalculateRoute(currentLoc);
        }

        var nextTarget = _destination.GetImmediateTarget();
        if (nextTarget != null)
        {
            var navPos = new PluginNavigationPosition(
                CellId: 0,
                EastWest: nextTarget.Coords.EW,
                NorthSouth: nextTarget.Coords.NS,
                Elevation: double.NaN,
                HeadingDegrees: 0f,
                IsOutdoor: true);
            var status = _host.Automation.Navigation.GoTo(navPos, (float)_settings.ArrivalDistance);
            if (status != PluginNavigationCommandStatus.Accepted)
            {
                _isNavigating = false;
                _host.Log.Warn($"GoArrow: Next route leg was {status}.");
                return;
            }

            _activeSequence = _host.Automation.Navigation.GoToReport.Sequence;
        }
    }

    public void Dispose()
    {
        Disable();
        StopNavigation();
    }
}