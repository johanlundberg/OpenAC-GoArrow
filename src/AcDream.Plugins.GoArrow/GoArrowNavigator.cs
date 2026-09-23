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
    private bool _pausedForOutdoorRoute;
    private bool _indoorWalk;
    private bool _indoorRouteLeg;
    private uint _resolvedIndoorPortalObjectId;
    private PluginNavigationPosition? _resolvedIndoorPortalPosition;
    private double _indoorRetryElapsed;
    private PluginNavigationPosition? _currentNavPosition;
    private long _activeSequence;
    private long _lastHandledReportRevision;
    private bool _navigationEventsSubscribed;
    private uint _activeInteractionObjectId;
    private long _lastActivationRevision;
    private long _lastTransitionRevision;
    private long _lastTransitionGeneration;
    private PluginNavigationPosition? _interactionOriginPosition;
    private bool _interactionSawPortalSpace;
    private double _legElapsed;
    private int _legRetries;
    private string _failureReason = string.Empty;
    private Guid _routeId;
    private int _legIndex;
    private long _transitionGeneration;
    private const string PluginOwner = "openac.goarrow";

    /// <summary>Whether the current route is paused for a portal or recall action.</summary>
    public bool WaitingForInteraction { get; private set; }

    /// <summary>Whether navigation is currently active.</summary>
    public bool IsNavigating => _isNavigating;

    public bool IsPlanningPath => _isNavigating
        && _host.Automation.Navigation.GoToReport is { State: PluginGoToState.Planning } report
        && report.Sequence == _activeSequence;

    public bool WaitingForIndoorPortal => _pausedForOutdoorRoute
        && _host.Automation.Navigation.Snapshot is { IsAvailable: true, IsPortalSpace: false, Position.IsOutdoor: false }
        && _destination.CurrentRoute is { StepCount: > 0 };

    /// <summary>Whether the current destination can be reached through the host's indoor pathfinder.</summary>
    public bool HasIndoorTarget
    {
        get
        {
            var snapshot = _host.Automation.Navigation.Snapshot;
            return CanNavigateIndoorTarget(snapshot);
        }
    }

    /// <summary>Last navigation status report from GoTo.</summary>
    public string LastReport => _host.Automation.Navigation.GoToReport.State.ToString();

    /// <summary>Whether the character is at the destination.</summary>
    public bool HasArrived { get; private set; }

    /// <summary>Latest recoverable failure diagnostic for the panel.</summary>
    public string FailureReason => _failureReason;
    public Guid RouteId => _routeId;
    public int LegIndex => _legIndex;
    public long TransitionGeneration => _transitionGeneration;

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

    /// <summary>Compute a route from the live position without driving the character.</summary>
    public bool PlanRoute()
    {
        if (_destination.TargetLocation is null || !_host.Automation.IsAvailable)
            return false;

        _resolvedIndoorPortalObjectId = 0;
        _resolvedIndoorPortalPosition = null;
        var snapshot = _host.Automation.Navigation.Snapshot;
        if (CanNavigateIndoorTarget(snapshot))
        {
            _currentNavPosition = snapshot.Position;
            _destination.ClearRoute();
            return true;
        }
        if (TryResolveIndoorPortal(snapshot))
        {
            _currentNavPosition = snapshot.Position;
            _destination.ClearRoute();
            return true;
        }
        if (_destination.TargetIndoorPosition is not null
            || _destination.TargetObjectPosition is { IsOutdoor: false })
        {
            _host.Log.Warn("GoArrow: Indoor destination is outside the current dungeon.");
            return false;
        }
        // The Atlas graph has outdoor map coordinates but no indoor cells or
        // floors. Reusing an indoor map coordinate would attach this route to
        // whichever outdoor entrance happens to be closest on the map.
        if (snapshot.IsAvailable && (snapshot.IsPortalSpace || !snapshot.Position.IsOutdoor))
        {
            _host.Log.Warn("GoArrow: Outdoor route planning is paused while indoors or in portal space.");
            return false;
        }
        if (snapshot.IsAvailable)
            _currentNavPosition = snapshot.Position;
        if (_currentNavPosition is null)
        {
            _host.Log.Warn("GoArrow: Current position unavailable; cannot calculate a route.");
            return false;
        }

        var currentLoc = new RouteFinding.Location(
            "Current Position",
            _currentNavPosition.Value.NorthSouth,
            _currentNavPosition.Value.EastWest);
        _destination.CalculateRoute(currentLoc);
        if (_destination.CurrentRoute is not { StepCount: > 0 })
        {
            _host.Log.Warn("GoArrow: No route found.");
            return false;
        }
        return true;
    }

    /// <summary>
    /// Start navigating to the current destination.
    /// </summary>
    public void StartNavigation()
    {
        if (!_settings.UseNavigationAutomation)
        {
            PlanRoute();
            return;
        }

        if (!_host.Automation.IsAvailable)
        {
            _host.Log.Warn("GoArrow: Navigation automation unavailable (not in world?)");
            return;
        }

        if (_isNavigating)
            StopNavigation();
        _pausedForOutdoorRoute = false;
        if (!PlanRoute())
            return;

        _isNavigating = false;
        HasArrived = false;
        WaitingForInteraction = false;
        _activeSequence = 0;
        _lastHandledReportRevision = 0;
        _lastTransitionRevision = 0;
        _lastTransitionGeneration = 0;
        _interactionOriginPosition = null;
        _interactionSawPortalSpace = false;
        _routeId = Guid.NewGuid();
        _legIndex = 0;
        _transitionGeneration = 0;
        if (HasIndoorTarget)
        {
            StartIndoorWalk();
            return;
        }
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
        _pausedForOutdoorRoute = false;
        _indoorWalk = false;
        _indoorRouteLeg = false;
        _resolvedIndoorPortalObjectId = 0;
        _resolvedIndoorPortalPosition = null;
        _indoorRetryElapsed = 0;
        HasArrived = false;
        WaitingForInteraction = false;
        _activeSequence = 0;
        _lastHandledReportRevision = 0;
        _activeInteractionObjectId = 0;
        _lastActivationRevision = 0;
        _lastTransitionRevision = 0;
        _lastTransitionGeneration = 0;
        _interactionOriginPosition = null;
        _interactionSawPortalSpace = false;
        _legElapsed = 0;
        _legRetries = 0;
        _failureReason = string.Empty;
        _routeId = Guid.Empty;
        _legIndex = 0;
        _transitionGeneration = 0;
    }

    /// <summary>
    /// Called on each Tick to check navigation progress.
    /// </summary>
    public void OnTick(double elapsed)
    {
        // A completed portal event can be missed if the host finishes the
        // transition before the interaction step is subscribed or published.
        // The live position is an independent signal that the portal worked.
        CheckInteractionPosition();

        if (_pausedForOutdoorRoute)
        {
            var snapshot = _host.Automation.Navigation.Snapshot;
            if (!snapshot.IsAvailable || snapshot.IsPortalSpace)
                return;
            if (!snapshot.Position.IsOutdoor)
            {
                _indoorRetryElapsed += Math.Max(0, elapsed);
                if (_indoorRetryElapsed >= 0.5)
                {
                    _indoorRetryElapsed = 0;
                    StartCurrentLeg();
                }
                return;
            }
            _pausedForOutdoorRoute = false;
            if (!PlanRoute())
                return;
            _failureReason = string.Empty;
            StartCurrentLeg();
        }

        if (!_isNavigating || _destination.TargetLocation == null || _currentNavPosition == null)
            return;

        if (_indoorWalk)
        {
            if (!CanNavigateIndoorTarget(_host.Automation.Navigation.Snapshot))
            {
                StopNavigation();
                _failureReason = "Indoor target is no longer in this dungeon.";
                return;
            }
            HandleNavigationReport(_host.Automation.Navigation.GoToReport);
            return;
        }

        var currentLoc = new RouteFinding.Location(
            "Current Position",
            _currentNavPosition.Value.NorthSouth,
            _currentNavPosition.Value.EastWest);

        _legElapsed += Math.Max(0, elapsed);
        var currentStep = _destination.CurrentRoute?.Steps.FirstOrDefault();
        if (currentStep is not null && _legElapsed > _settings.InteractionTimeoutSeconds)
        {
            _failureReason = $"Timed out on {currentStep.Kind.ToString().ToLowerInvariant()} '{currentStep.Via}'.";
            if (_legRetries < _settings.MaxNavigationRetries)
            {
                _legRetries++;
                _legElapsed = 0;
                StartCurrentLeg();
            }
            else
            {
                _isNavigating = false;
                WaitingForInteraction = true;
                _host.Automation.Chat.PostSystemMessage($"GoArrow: {_failureReason}");
            }
            return;
        }

        _destination.UpdateGuidance(currentLoc);

        // Poll GoTo report for state changes
        HandleNavigationReport(_host.Automation.Navigation.GoToReport);
    }

    /// <summary>
    /// Resume a route after a portal transition completes or the user confirms
    /// a manual portal or recall interaction.
    /// </summary>
    public void ResumeAfterInteraction()
    {
        if (!WaitingForInteraction || _destination.CurrentRoute is not { StepCount: > 0 } route
            || route.Steps[0].Kind == RouteStepKind.Travel)
            return;

        _destination.AdvanceStep();
        WaitingForInteraction = false;
        _activeInteractionObjectId = 0;
        _interactionOriginPosition = null;
        _interactionSawPortalSpace = false;
        _resolvedIndoorPortalObjectId = 0;
        _resolvedIndoorPortalPosition = null;
        _legIndex++;
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

        var snapshot = _host.Automation.Navigation.Snapshot;
        if (snapshot.IsAvailable && snapshot.IsPortalSpace)
        {
            _isNavigating = false;
            _pausedForOutdoorRoute = true;
            _failureReason = "Waiting for portal transition.";
            return;
        }

        if (snapshot.IsAvailable && !snapshot.Position.IsOutdoor)
        {
            if (step.Kind == RouteStepKind.Travel
                && _destination.CurrentRoute is { Steps.Count: > 1 } route
                && route.Steps[1].Kind == RouteStepKind.Portal
                && TryResolveIndoorPortal(snapshot, step.To.Name))
            {
                _indoorRouteLeg = true;
                _pausedForOutdoorRoute = false;
                StartIndoorWalk();
                if (!_isNavigating)
                {
                    _indoorRouteLeg = false;
                    _pausedForOutdoorRoute = true;
                }
                return;
            }
            if (step.Kind == RouteStepKind.Portal
                && (TryResolveIndoorPortal(snapshot, step.Via) || _resolvedIndoorPortalObjectId != 0))
            {
                _pausedForOutdoorRoute = false;
                _transitionGeneration++;
                TryStartInteraction(step);
                return;
            }
            _isNavigating = false;
            _pausedForOutdoorRoute = true;
            _failureReason = "Waiting for the next indoor portal.";
            return;
        }

        if (step.Kind != RouteStepKind.Travel)
        {
            _transitionGeneration++;
            TryStartInteraction(step);
            return;
        }

        var immediateTarget = step.To;
        _isNavigating = true;
        _legElapsed = 0;
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
        _lastHandledReportRevision = 0;
        _host.Log.Info($"GoArrow: Navigating to {immediateTarget.Name} at ({navPos.EastWest:F2}, {navPos.NorthSouth:F2})");
    }

    private bool CanNavigateIndoorTarget(PluginNavigationSnapshot snapshot)
    {
        PluginNavigationPosition? targetPosition = _destination.TargetIndoorPosition
            ?? _resolvedIndoorPortalPosition;
        if (_destination.Kind == GoArrowDestinationKind.Object && !_destination.TargetUnavailable
            && _destination.TargetObjectId is > 0)
            targetPosition = _destination.TargetObjectPosition;
        return snapshot.IsAvailable && !snapshot.IsPortalSpace && !snapshot.Position.IsOutdoor
            && targetPosition is { IsOutdoor: false } target
            && snapshot.Position.CellId != 0 && target.CellId != 0
            && (snapshot.Position.CellId & 0xFFFF0000u) == (target.CellId & 0xFFFF0000u);
    }

    private bool TryResolveIndoorPortal(PluginNavigationSnapshot snapshot)
    {
        if (!snapshot.IsAvailable || snapshot.IsPortalSpace || snapshot.Position.IsOutdoor
            || snapshot.Position.CellId == 0 || _destination.Kind != GoArrowDestinationKind.Location
            || _destination.TargetIndoorPosition is not null)
            return false;

        return TryResolveIndoorPortal(snapshot, _destination.TargetName);
    }

    private bool TryResolveIndoorPortal(PluginNavigationSnapshot snapshot, string name)
    {
        if (!snapshot.IsAvailable || snapshot.IsPortalSpace || snapshot.Position.IsOutdoor
            || snapshot.Position.CellId == 0)
            return false;

        string destinationName = PortalDestinationName(name);
        if (destinationName.Length == 0)
            return false;
        uint landblock = snapshot.Position.CellId & 0xFFFF0000u;
        PluginWorldObject portal = _host.Automation.Objects.CaptureObjects()
            .Where(obj => obj.ObjectId != 0 && obj.HasPosition && obj.CanActivate
                && (obj.Capabilities & PluginObjectCapabilities.Portal) != 0
                && !obj.Position.IsOutdoor && (obj.Position.CellId & 0xFFFF0000u) == landblock
                && PortalDestinationName(obj.Name).Equals(destinationName, StringComparison.OrdinalIgnoreCase))
            .OrderBy(obj => snapshot.Position.HorizontalDistanceMeters(obj.Position))
            .FirstOrDefault();
        if (portal.ObjectId == 0)
            return false;
        _resolvedIndoorPortalObjectId = portal.ObjectId;
        _resolvedIndoorPortalPosition = portal.Position;
        return true;
    }

    private static string PortalDestinationName(string name)
    {
        string value = name.Trim();
        if (value.StartsWith("Town Network Portal to ", StringComparison.OrdinalIgnoreCase))
            value = value["Town Network Portal to ".Length..];
        else if (value.StartsWith("Portal to ", StringComparison.OrdinalIgnoreCase))
            value = value["Portal to ".Length..];
        else if (value.StartsWith("Portal for ", StringComparison.OrdinalIgnoreCase))
            value = value["Portal for ".Length..];
        else if (value.StartsWith("Town Network ", StringComparison.OrdinalIgnoreCase)
            && value.LastIndexOf(" to ", StringComparison.OrdinalIgnoreCase) is var toIndex and >= 0)
            value = value[(toIndex + 4)..];
        if (value.EndsWith(" Town Network Portal", StringComparison.OrdinalIgnoreCase))
            value = value[..^" Town Network Portal".Length];
        else if (value.EndsWith(" Portal", StringComparison.OrdinalIgnoreCase))
            value = value[..^" Portal".Length];
        return value.Trim();
    }

    private void StartIndoorWalk()
    {
        PluginNavigationCommandStatus status;
        if (_destination.Kind == GoArrowDestinationKind.Object
            && _destination.TargetObjectId is { } objectId)
            status = _host.Automation.Navigation.GoTo(objectId, (float)_settings.ArrivalDistance);
        else if (_resolvedIndoorPortalObjectId != 0)
            status = _host.Automation.Navigation.GoTo(_resolvedIndoorPortalObjectId, (float)_settings.ArrivalDistance);
        else if (_destination.TargetIndoorPosition is { } position)
            status = _host.Automation.Navigation.GoTo(position, (float)_settings.ArrivalDistance);
        else
            return;
        if (status != PluginNavigationCommandStatus.Accepted)
        {
            _failureReason = $"Indoor navigation request was {status}.";
            _host.Log.Warn($"GoArrow: {_failureReason}");
            return;
        }
        _indoorWalk = true;
        _isNavigating = true;
        _failureReason = string.Empty;
        _activeSequence = _host.Automation.Navigation.GoToReport.Sequence;
        _lastHandledReportRevision = 0;
        _host.Log.Info($"GoArrow: Navigating indoors to '{_destination.TargetName}'.");
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

        if (_indoorWalk && report.State == PluginGoToState.ArrivedWithoutSight)
        {
            _lastHandledReportRevision = report.Revision;
            _isNavigating = false;
            _indoorWalk = false;
            _failureReason = report.Reason ?? "Indoor destination was not reachable.";
            _host.Automation.Chat.PostSystemMessage($"GoArrow: {_failureReason}");
            return;
        }

        if (report.State is PluginGoToState.Arrived or PluginGoToState.ArrivedWithoutSight)
        {
            _lastHandledReportRevision = report.Revision;
            if (_indoorWalk)
            {
                _isNavigating = false;
                _indoorWalk = false;
                if (_indoorRouteLeg)
                {
                    _indoorRouteLeg = false;
                    HandleWaypointReached();
                    return;
                }
                HasArrived = true;
                _activeSequence = 0;
                _host.Automation.Chat.PostSystemMessage($"GoArrow: Arrived at '{_destination.TargetName}'.");
                return;
            }
            HandleWaypointReached();
            return;
        }

        if (report.State is PluginGoToState.NoRoute
            or PluginGoToState.Blocked
            or PluginGoToState.Interrupted
            or PluginGoToState.Lost)
        {
            _lastHandledReportRevision = report.Revision;
            if (!_indoorWalk && report.State == PluginGoToState.Blocked && report.BlockedByObjectId != 0)
            {
                if (_host.Automation.Objects.TryGet(report.BlockedByObjectId, out PluginWorldObject blocked)
                    && blocked.ObjectClass == PluginObjectClass.Door && !blocked.IsDoorOpen && blocked.CanActivate)
                {
                    _activeInteractionObjectId = blocked.ObjectId;
                    WaitingForInteraction = true;
                    _isNavigating = false;
                    _host.Automation.Objects.Activate(blocked.ObjectId);
                    return;
                }
            }
            _isNavigating = false;
            _indoorWalk = false;
            _indoorRouteLeg = false;
            _failureReason = report.Reason ?? report.State.ToString();
            _host.Log.Warn($"GoArrow: Navigation stopped with state {report.State}: {_failureReason}.");
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
        _legIndex++;
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
        var snapshot = _host.Automation.Navigation.Snapshot;
        _interactionOriginPosition = snapshot.IsAvailable ? snapshot.Position : null;
        _interactionSawPortalSpace = false;
        _activeInteractionObjectId = step.ObjectId != 0 ? step.ObjectId
            : step.Kind == RouteStepKind.Portal ? _resolvedIndoorPortalObjectId : 0;
        if (_activeInteractionObjectId == 0)
        {
            PluginObjectCapabilities required = step.Kind == RouteStepKind.Portal
                ? PluginObjectCapabilities.Portal
                : PluginObjectCapabilities.Interactable;
            var candidate = _host.Automation.Objects.CaptureObjects()
                .Where(obj => obj.CanActivate && (obj.Capabilities & required) != 0)
                .Select(obj => new
                {
                    Object = obj,
                    Distance = obj.HasPosition
                        ? new RouteFinding.Coordinates(obj.Position.NorthSouth, obj.Position.EastWest)
                            .DistanceTo(step.From.Coords)
                        : double.PositiveInfinity,
                    NameMatches = !string.IsNullOrWhiteSpace(step.Via)
                        && obj.Name.Contains(step.Via, StringComparison.OrdinalIgnoreCase)
                })
                .Where(candidate => candidate.NameMatches
                    ? !candidate.Object.HasPosition || candidate.Distance <= 1.0
                    : step.Kind == RouteStepKind.Portal && candidate.Distance <= 0.1)
                .OrderBy(candidate => candidate.NameMatches ? 0 : 1)
                .ThenBy(candidate => candidate.Distance)
                .Select(candidate => candidate.Object)
                .FirstOrDefault();
            _activeInteractionObjectId = candidate.ObjectId;
        }

        if (_activeInteractionObjectId == 0)
        {
            WaitingForInteraction = true;
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Could not identify {step.Kind.ToString().ToLowerInvariant()} '{step.Via}'. Complete it manually, then use /go resume.");
            return;
        }

        WaitingForInteraction = true;
        var result = _host.Automation.Objects.Activate(_activeInteractionObjectId);
        if (!result.Accepted)
        {
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Interaction with '{step.Via}' was not accepted ({result.Status}); use /go resume if completed manually.");
            return;
        }
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
        if (_destination.CurrentRoute is not { StepCount: > 0 } route)
            return;

        if (route.Steps[0].Kind == RouteStepKind.Travel)
        {
            WaitingForInteraction = false;
            _activeInteractionObjectId = 0;
            StartCurrentLeg();
        }
        // Portal and recall activation only confirms that the interaction
        // succeeded. The route advances when the world transition completes.
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
        if (!WaitingForInteraction || !transition.IsCompleted
            || transition.Kind == PluginPortalTransitionKind.Login
            || _destination.CurrentRoute is not { StepCount: > 0 } route
            || route.Steps[0].Kind is not (RouteStepKind.Portal or RouteStepKind.Recall))
            return;
        if (transition.Revision > 0 && transition.Revision <= _lastTransitionRevision)
            return;
        if (transition.Generation > 0 && transition.Generation <= _lastTransitionGeneration)
            return;
        if (transition.Revision == 0 && transition.Generation == 0)
            return;
        _lastTransitionRevision = transition.Revision;
        _lastTransitionGeneration = transition.Generation;
        ResumeAfterInteraction();
    }

    private void CheckInteractionPosition()
    {
        if (!WaitingForInteraction
            || _destination.CurrentRoute is not { StepCount: > 0 } route
            || route.Steps[0].Kind is not (RouteStepKind.Portal or RouteStepKind.Recall)
            || _interactionOriginPosition is not { } origin)
            return;

        var snapshot = _host.Automation.Navigation.Snapshot;
        if (!snapshot.IsAvailable)
            return;
        if (snapshot.IsPortalSpace)
        {
            _interactionSawPortalSpace = true;
            return;
        }

        var current = snapshot.Position;
        bool changedWorld = current.IsOutdoor != origin.IsOutdoor
            || (current.CellId != 0 && origin.CellId != 0
                && (current.CellId & 0xFFFF0000u) != (origin.CellId & 0xFFFF0000u));
        bool movedAfterPortalSpace = _interactionSawPortalSpace
            && (current.CellId != origin.CellId
                || current.HorizontalDistanceMeters(origin) > 20);
        if (!changedWorld && !movedAfterPortalSpace)
            return;

        _host.Log.Info($"GoArrow: Completed '{route.Steps[0].Via}' from the live position after portal transition.");
        ResumeAfterInteraction();
    }

    public void Dispose()
    {
        Disable();
        StopNavigation();
    }
}
