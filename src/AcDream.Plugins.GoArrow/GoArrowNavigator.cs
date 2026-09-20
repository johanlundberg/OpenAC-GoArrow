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
    private uint _activeInteractionObjectId;
    private long _lastActivationRevision;
    private long _lastTransitionRevision;
    private const string PluginOwner = "openac.goarrow";

    /// <summary>Whether the current route is paused for a portal or recall action.</summary>
    public bool WaitingForInteraction { get; private set; }

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
        _host.Events.ActivationCompleted += OnActivationCompleted;
        _host.Events.PortalTransition += OnPortalTransition;
        _host.Events.ObjectChanged += OnObjectChanged;
        _navigationEventsSubscribed = true;
    }

    public void Disable()
    {
        if (!_navigationEventsSubscribed)
            return;

        _host.Events.NavigationChanged -= OnNavigationChanged;
        _host.Events.ActivationCompleted -= OnActivationCompleted;
        _host.Events.PortalTransition -= OnPortalTransition;
        _host.Events.ObjectChanged -= OnObjectChanged;
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

        _isNavigating = false;
        HasArrived = false;
        WaitingForInteraction = false;
        _activeSequence = 0;
        _lastHandledReportRevision = 0;
        StartCurrentLeg();
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
        WaitingForInteraction = false;
        _activeSequence = 0;
        _lastHandledReportRevision = 0;
        _activeInteractionObjectId = 0;
        _lastActivationRevision = 0;
        _lastTransitionRevision = 0;
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

    /// <summary>
    /// Resume a route after the user has completed the current portal or recall
    /// interaction. OpenAC does not currently expose a generic interaction API,
    /// so the action is intentionally explicit rather than pretending that
    /// arrival at a portal completes the transition.
    /// </summary>
    public void ResumeAfterInteraction()
    {
        if (!WaitingForInteraction || _destination.CurrentRoute is not { StepCount: > 0 })
            return;

        _destination.AdvanceStep();
        WaitingForInteraction = false;
        if (_destination.CurrentRoute.StepCount == 0)
        {
            HasArrived = true;
            _host.Automation.Chat.PostSystemMessage($"GoArrow: Arrived at '{_destination.TargetName}'.");
            return;
        }

        StartCurrentLeg();
    }

    private void StartCurrentLeg()
    {
        var step = _destination.CurrentRoute?.Steps.FirstOrDefault();
        if (step is null)
            return;

        if (step.Kind != RouteStepKind.Travel)
        {
            TryStartInteraction(step);
            return;
        }

        var immediateTarget = step.To;
        _isNavigating = true;
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

    private void OnNavigationChanged(PluginGoToReport report)
    {
        if (_isNavigating)
            HandleNavigationReport(report);
    }

    private void HandleNavigationReport(PluginGoToReport report)
    {
        // Reports from another owner or an earlier GoTo must not advance this route.
        if (report.Owner is not null && !report.Owner.Equals(PluginOwner, StringComparison.OrdinalIgnoreCase))
            return;
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

        // Continue to the next leg. Keep the planned route intact so a
        // portal/recall step is not lost during recalculation.
        StartCurrentLeg();
    }

    private void TryStartInteraction(RouteStep step)
    {
        _isNavigating = false;
        _activeInteractionObjectId = step.ObjectId;
        if (_activeInteractionObjectId == 0)
        {
            PluginObjectCapabilities required = step.Kind == RouteStepKind.Portal
                ? PluginObjectCapabilities.Portal
                : PluginObjectCapabilities.Interactable;
            var candidate = _host.Automation.Objects.CaptureObjects()
                .FirstOrDefault(obj => (obj.Capabilities & required) != 0
                    && (string.IsNullOrWhiteSpace(step.Via)
                        || obj.Name.Contains(step.Via, StringComparison.OrdinalIgnoreCase)));
            _activeInteractionObjectId = candidate.ObjectId;
        }

        if (_activeInteractionObjectId == 0)
        {
            WaitingForInteraction = true;
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Could not identify {step.Kind.ToString().ToLowerInvariant()} '{step.Via}'. Complete it manually, then use /go resume.");
            return;
        }

        var result = _host.Automation.Objects.Activate(_activeInteractionObjectId);
        if (!result.Accepted)
        {
            WaitingForInteraction = true;
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Interaction with '{step.Via}' was not accepted ({result.Status}); use /go resume if completed manually.");
            return;
        }
        WaitingForInteraction = true;
        _host.Automation.Chat.PostSystemMessage($"GoArrow: Activating '{step.Via}'.");
    }

    private void OnActivationCompleted(PluginActivationCompletion completion)
    {
        if (!WaitingForInteraction || completion.ObjectId != _activeInteractionObjectId
            || completion.Revision == 0 || completion.Revision <= _lastActivationRevision)
            return;
        _lastActivationRevision = completion.Revision;
        if (!completion.IsSuccess)
        {
            _host.Log.Warn($"GoArrow: Interaction failed ({completion.Outcome}) for object 0x{completion.ObjectId:X8}.");
            _host.Automation.Chat.PostSystemMessage($"GoArrow: Interaction failed ({completion.Outcome}); use /go resume to retry.");
            return;
        }
        ResumeAfterInteraction();
    }

    private void OnObjectChanged(PluginObjectChange change)
    {
        if (_destination.Kind != GoArrowDestinationKind.Object
            || _destination.TargetObjectId != change.ObjectId)
            return;
        if (change.Kind == PluginObjectChangeKind.Released || change.Current is not { } current)
        {
            _destination.MarkObjectUnavailable();
            return;
        }
        _destination.UpdateObject(current);
    }

    private void OnPortalTransition(PluginPortalTransition transition)
    {
        if (!WaitingForInteraction || transition.Revision == 0
            || transition.Revision <= _lastTransitionRevision || !transition.IsCompleted)
            return;
        _lastTransitionRevision = transition.Revision;
        ResumeAfterInteraction();
    }

    public void Dispose()
    {
        Disable();
        StopNavigation();
    }
}