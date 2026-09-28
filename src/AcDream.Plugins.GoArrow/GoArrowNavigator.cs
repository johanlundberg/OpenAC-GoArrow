using AcDream.Plugin.Abstractions;
using AcDream.Plugins.GoArrow.RouteFinding;
using System.Diagnostics;

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
    private bool _indoorSurfaceExitLeg;
    private bool _waitingForSurfaceExit;
    private uint _pendingPortalIdentificationObjectId;
    private long _pendingPortalIdentificationStarted;
    private readonly HashSet<uint> _portalIdentificationAttempted = [];
    private bool _observedIndoorRoute;
    private readonly DungeonPortalSearch _dungeonPortalSearch;
    internal DungeonTraversalStore DungeonTraversals { get; }
    private double _traversalSaveElapsed;
    private bool _exploringDungeonExit;
    private PluginNavigationPosition? _walkingPortalOrigin;
    private bool _walkingPortalSawTransit;
    private PluginNavigationPosition? _landscapeWalkPosition;
    private uint _outdoorPortalRetryObjectId;
    private bool _outdoorDetourActive;
    private int _outdoorDetourAttempts;
    private readonly List<PluginNavigationPosition> _outdoorDetourTried = [];
    private Coordinates _outdoorDetourGoal = Coordinates.NoCoordinates;
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
    private string _failureReason = string.Empty;
    private Guid _routeId;
    private int _legIndex;
    private long _transitionGeneration;
    private const string PluginOwner = "openac.goarrow";

    /// <summary>Whether the current route is paused for a portal or recall action.</summary>
    public bool WaitingForInteraction { get; private set; }

    /// <summary>Whether navigation is currently active.</summary>
    public bool IsNavigating => _isNavigating;

    public bool CanResumeNavigation =>
        _destination.HasDestination
        && !_isNavigating
        && !HasArrived
        && (WaitingForInteraction || (_settings.AutoNavigate && !_pausedForOutdoorRoute));

    public bool IsPlanningPath =>
        _isNavigating
        && _host.Automation.Navigation.GoToReport is { State: PluginGoToState.Planning } report
        && report.Sequence == _activeSequence;

    public bool WaitingForIndoorPortal =>
        _pausedForOutdoorRoute
        && !_waitingForSurfaceExit
        && _host.Automation.Navigation.Snapshot
            is { IsAvailable: true, IsPortalSpace: false, Position.IsOutdoor: false }
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
    internal IReadOnlyList<(string Label, PluginNavigationPosition Position)> DungeonRouteWaypoints
    {
        get
        {
            if (_host.Automation.Navigation.Snapshot is not
                { IsAvailable: true, IsPortalSpace: false, Position.IsOutdoor: false }
                || _destination.CurrentRoute is not { StepCount: > 0 } route
                || !NeedsDungeonExit(route.Steps[0]))
                return [];
            if (_dungeonPortalSearch.RemainingRecordedWaypoints > 0)
            {
                var points = _dungeonPortalSearch.RecordedWaypoints;
                var rows = points.Select((p, i) =>
                    ($"Recorded dungeon walk {i + 1}: cell {p.CellId:X8}, floor {p.Elevation * 240:0} m", p))
                    .ToList();
                int remaining = _dungeonPortalSearch.RemainingRecordedWaypoints - points.Count;
                if (remaining > 0)
                    rows.Add(($"... {remaining} more recorded waypoints", default));
                return rows;
            }
            return _exploringDungeonExit
                ? [($"Dungeon {_dungeonPortalSearch.Reason}", _dungeonPortalSearch.CurrentTarget)]
                : [];
        }
    }
    public Guid RouteId => _routeId;
    public int LegIndex => _legIndex;
    public long TransitionGeneration => _transitionGeneration;

    public GoArrowNavigator(
        IPluginHost host,
        GoArrowDestination destination,
        GoArrowSettings settings
    )
    {
        _host = host;
        _destination = destination;
        _settings = settings;
        DungeonTraversals = new(host.Storage);
        DungeonTraversals.Load();
        _dungeonPortalSearch = new(DungeonTraversals);
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

        _outdoorPortalRetryObjectId = 0;
        _resolvedIndoorPortalObjectId = 0;
        _resolvedIndoorPortalPosition = null;
        var snapshot = _host.Automation.Navigation.Snapshot;
        if (snapshot.IsAvailable && !snapshot.IsPortalSpace && !snapshot.Position.IsOutdoor
            && _destination.CurrentRoute is null)
            _destination.TryPlanDungeonExit(snapshot.Position.CellId);
        if (snapshot.Position.IsOutdoor)
            _dungeonPortalSearch.Reset();
        if (snapshot.IsAvailable && !snapshot.IsPortalSpace && !snapshot.Position.IsOutdoor
            && _destination.CurrentRoute?.Steps.FirstOrDefault() is { } pending
            && NeedsDungeonExit(pending))
        {
            _currentNavPosition = snapshot.Position;
            if (TryResolveRouteDungeonExit(snapshot, pending))
            {
                _indoorSurfaceExitLeg = true;
                return true;
            }
            _pausedForOutdoorRoute = true;
            _failureReason = IndoorPortalSearchReason(snapshot, pending);
            return false;
        }
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
        if (
            snapshot.IsAvailable
            && !snapshot.IsPortalSpace
            && !snapshot.Position.IsOutdoor
            && _destination.Kind == GoArrowDestinationKind.Location
            && _destination.TargetIndoorPosition is null
            && TryResolveSurfaceExit(snapshot)
        )
        {
            _currentNavPosition = snapshot.Position;
            _destination.ClearRoute();
            _indoorSurfaceExitLeg = true;
            return true;
        }
        if (
            _destination.TargetIndoorPosition is not null
            || _destination.TargetObjectPosition is { IsOutdoor: false }
        )
        {
            _host.Log.Warn("GoArrow: Indoor destination is outside the current dungeon.");
            return false;
        }
        // The Atlas graph has outdoor map coordinates but no indoor cells or
        // floors. Reusing an indoor map coordinate would attach this route to
        // whichever outdoor entrance happens to be closest on the map.
        if (snapshot.IsAvailable && (snapshot.IsPortalSpace || !snapshot.Position.IsOutdoor))
        {
            _host.Log.Warn(
                "GoArrow: Outdoor route planning is paused while indoors or in portal space."
            );
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
            _currentNavPosition.Value.EastWest
        );
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
        _indoorSurfaceExitLeg = false;
        _waitingForSurfaceExit = false;
        _observedIndoorRoute = false;
        var startSnapshot = _host.Automation.Navigation.Snapshot;
        bool keepChosenRoute = startSnapshot.IsAvailable && !startSnapshot.IsPortalSpace
            && _destination.CanKeepSelectedAlternative(startSnapshot.Position);
        if (!keepChosenRoute && !PlanRoute())
            return;
        if (keepChosenRoute)
            _currentNavPosition = startSnapshot.Position;

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
        _failureReason = string.Empty;
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
        _outdoorDetourActive = false;
        _outdoorDetourAttempts = 0;
        _outdoorDetourTried.Clear();
        _outdoorDetourGoal = Coordinates.NoCoordinates;
        DungeonTraversals.Save();
        DungeonTraversals.BreakTrace();
        _exploringDungeonExit = false;
        if (_isNavigating && _host.Automation.IsAvailable)
        {
            _host.Automation.Navigation.StopGoTo();
        }

        _isNavigating = false;
        _pausedForOutdoorRoute = false;
        _indoorWalk = false;
        _indoorRouteLeg = false;
        _indoorSurfaceExitLeg = false;
        _waitingForSurfaceExit = false;
        _pendingPortalIdentificationObjectId = 0;
        _pendingPortalIdentificationStarted = 0;
        _portalIdentificationAttempted.Clear();
        _observedIndoorRoute = false;
        _landscapeWalkPosition = null;
        _walkingPortalOrigin = null;
        _walkingPortalSawTransit = false;
        _dungeonPortalSearch.Reset();
        _resolvedIndoorPortalObjectId = 0;
        _outdoorPortalRetryObjectId = 0;
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

        var live = _host.Automation.Navigation.Snapshot;
        if (live.IsMoving || ((_exploringDungeonExit || _indoorSurfaceExitLeg)
            && _host.Automation.Navigation.GoToReport.State == PluginGoToState.Walking))
            DungeonTraversals.Observe(live);
        else if (!live.IsAvailable || live.IsPortalSpace || live.Position.IsOutdoor)
            DungeonTraversals.BreakTrace();
        _traversalSaveElapsed += Math.Max(0, elapsed);
        if (_traversalSaveElapsed >= 15)
        {
            DungeonTraversals.Save();
            _traversalSaveElapsed = 0;
        }
        if (_walkingPortalOrigin is not null && live.IsPortalSpace)
            _walkingPortalSawTransit = true;
        if (TryCompleteWalkingPortal(live))
            return;
        UpdateLandscapeWalk(live);
        if (_indoorWalk && (_indoorRouteLeg || _indoorSurfaceExitLeg)
            && live.IsAvailable && live.IsPortalSpace)
        {
            PauseForPortalExit();
            return;
        }
        if (live.IsAvailable && !live.IsPortalSpace)
        {
            if (!live.Position.IsOutdoor && IsIndoorPortalStep() && _outdoorPortalRetryObjectId == 0
                && _landscapeWalkPosition is null)
                _observedIndoorRoute = true;
            else if (
                live.Position.IsOutdoor
                && (
                    _indoorRouteLeg
                    || _indoorSurfaceExitLeg
                    || _waitingForSurfaceExit
                    || _observedIndoorRoute
                    || (_routeId != Guid.Empty && IsNearTownNetworkExit(live.Position))
                )
            )
            {
                _observedIndoorRoute = false;
                if (IsIndoorPortalStep() || _indoorSurfaceExitLeg || _waitingForSurfaceExit)
                {
                    // The character can enter the portal before GoTo reports
                    // arrival at its object. A Lost report then leaves the
                    // old indoor walk step at the head of the route.
                    if (_isNavigating)
                        _host.Automation.Navigation.StopGoTo();
                    _isNavigating = false;
                    _indoorWalk = false;
                    _indoorRouteLeg = false;
                    _indoorSurfaceExitLeg = false;
                    _waitingForSurfaceExit = false;
                    WaitingForInteraction = false;
                    _pausedForOutdoorRoute = false;
                    _activeSequence = 0;
                    if (PlanRoute())
                    {
                        _failureReason = string.Empty;
                        StartCurrentLeg();
                    }
                    return;
                }
            }
        }

        if (_exploringDungeonExit)
        {
            var pending = _destination.CurrentRoute?.Steps.FirstOrDefault();
            if (pending is not null && TryResolveRouteDungeonExit(live, pending))
            {
                _exploringDungeonExit = false;
                _isNavigating = false;
                _host.Automation.Navigation.StopGoTo();
                StartCurrentLeg();
            }
            else if (!_dungeonPortalSearch.SafeToContinue(_host.Automation))
            {
                _dungeonPortalSearch.CompleteTarget(live.Position, false);
                _exploringDungeonExit = false;
                _isNavigating = false;
                _host.Automation.Navigation.StopGoTo();
                _pausedForOutdoorRoute = true;
                _indoorRetryElapsed = 0;
                _failureReason = _dungeonPortalSearch.Reason;
            }
            else
                HandleNavigationReport(_host.Automation.Navigation.GoToReport);
            return;
        }

        if (_pausedForOutdoorRoute)
        {
            var snapshot = _host.Automation.Navigation.Snapshot;
            if (!snapshot.IsAvailable || snapshot.IsPortalSpace)
                return;
            if (!snapshot.Position.IsOutdoor)
            {
                if (_waitingForSurfaceExit)
                    return;
                _indoorRetryElapsed += Math.Max(0, elapsed);
                if (_indoorRetryElapsed >= 0.5)
                {
                    _indoorRetryElapsed = 0;
                    StartCurrentLeg();
                }
                return;
            }
            _waitingForSurfaceExit = false;
            _indoorSurfaceExitLeg = false;
            WaitingForInteraction = false;
            _activeInteractionObjectId = 0;
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
                _host.Automation.Chat.PostSystemMessage($"GoArrow: {_failureReason}");
                return;
            }
            HandleNavigationReport(_host.Automation.Navigation.GoToReport);
            return;
        }

        var currentLoc = new RouteFinding.Location(
            "Current Position",
            _currentNavPosition.Value.NorthSouth,
            _currentNavPosition.Value.EastWest
        );

        _destination.UpdateGuidance(currentLoc);

        // Poll GoTo report for state changes
        HandleNavigationReport(_host.Automation.Navigation.GoToReport);
    }

    /// <summary>
    /// Resume a route after a portal transition completes or the user confirms
    /// a manual portal or recall interaction.
    /// </summary>
    public void ResumeNavigation()
    {
        if (!CanResumeNavigation)
            return;
        if (WaitingForInteraction)
        {
            ResumeAfterInteraction(startNextLeg: _settings.AutoNavigate);
            return;
        }

        var snapshot = _host.Automation.Navigation.Snapshot;
        var report = _host.Automation.Navigation.GoToReport;
        if (snapshot.IsAvailable && !snapshot.IsPortalSpace && !snapshot.Position.IsOutdoor
            && report.State == PluginGoToState.Lost
            && report.Reason?.Contains("portal space", StringComparison.OrdinalIgnoreCase) == true
            && _destination.CurrentRoute is { Steps.Count: > 1 } interruptedApproach
            && interruptedApproach.Steps[0].Kind == RouteStepKind.Travel
            && interruptedApproach.Steps[1].Kind == RouteStepKind.Portal
            && !interruptedApproach.Steps[0].From.Name.Contains(" arrival (", StringComparison.OrdinalIgnoreCase))
        {
            var origin = interruptedApproach.Steps[0].From.Coords;
            _walkingPortalOrigin = new PluginNavigationPosition(0, origin.EW, origin.NS, double.NaN, 0, true);
            _walkingPortalSawTransit = true;
            if (TryCompleteWalkingPortal(snapshot))
                return;
        }
        if (
            snapshot.IsAvailable
            && !snapshot.IsPortalSpace
            && !snapshot.Position.IsOutdoor
            && (IsIndoorPortalStep()
                || (_destination.CurrentRoute?.Steps.FirstOrDefault() is { } pending
                    && NeedsDungeonExit(pending)))
        )
        {
            var route = _destination.CurrentRoute!;
            if (
                route.Steps[0].Kind == RouteStepKind.Portal
                && route.Steps[0].To.HasCoordinates
                && new Coordinates(
                    snapshot.Position.NorthSouth,
                    snapshot.Position.EastWest
                ).DistanceTo(route.Steps[0].To.Coords) * 240
                    <= 250
            )
            {
                // Stop can clear a pending transition before the entry portal
                // is marked complete. The live indoor position is its arrival.
                _destination.AdvanceStep();
                if (_destination.CurrentRoute?.StepCount == 0)
                {
                    HasArrived = true;
                    _host.Automation.Chat.PostSystemMessage(
                        $"GoArrow: Arrived at '{_destination.TargetName}'."
                    );
                    return;
                }
            }
            _routeId = Guid.NewGuid();
            _legIndex = 0;
            _observedIndoorRoute = true;
            _pausedForOutdoorRoute = false;
            _failureReason = string.Empty;
            HasArrived = false;
            _activeSequence = 0;
            _lastHandledReportRevision = 0;
            StartCurrentLeg();
            return;
        }

        StartNavigation();
    }

    public void ResumeAfterInteraction(bool startNextLeg = true)
    {
        if (
            !WaitingForInteraction
            || _destination.CurrentRoute is not { StepCount: > 0 } route
            || route.Steps[0].Kind == RouteStepKind.Travel
        )
            return;

        _destination.AdvanceStep();
        WaitingForInteraction = false;
        _activeInteractionObjectId = 0;
        _outdoorPortalRetryObjectId = 0;
        _interactionOriginPosition = null;
        _interactionSawPortalSpace = false;
        _resolvedIndoorPortalObjectId = 0;
        _resolvedIndoorPortalPosition = null;
        _legIndex++;
        if (_destination.CurrentRoute.StepCount == 0)
        {
            HasArrived = true;
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Arrived at '{_destination.TargetName}'."
            );
            return;
        }

        if (startNextLeg)
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

        UpdateLandscapeWalk(snapshot);
        if (snapshot.IsAvailable && !snapshot.Position.IsOutdoor && _landscapeWalkPosition is null)
        {
            // An envcell can be a cave or room connected to the landscape.
            // Continue to the nearby Atlas entrance using its live object.
            if (step.Kind == RouteStepKind.Travel
                && !step.To.Name.StartsWith("Town Network (", StringComparison.OrdinalIgnoreCase)
                && TryStartOutdoorPortalWalk())
                return;
            if (step.Kind == RouteStepKind.Portal && IsNearRouteEntrance(snapshot, step.From))
            {
                var entrance = FindRouteEntrancePortal(step.From, snapshot, false);
                if (entrance.ObjectId != 0)
                {
                    _outdoorPortalRetryObjectId = entrance.ObjectId;
                    _pausedForOutdoorRoute = false;
                    TryStartInteraction(step);
                    return;
                }
            }
            if (_routeId != Guid.Empty
                && step.Kind == RouteStepKind.Portal
                && step.Via.StartsWith("Town Network Portal(", StringComparison.OrdinalIgnoreCase)
                && !IsNearRouteEntrance(snapshot, step.From))
            {
                // These Atlas records lead from a town into Town Network.
                // Being indoors away from the entrance means entry completed,
                // transition notification was missed or Stop cleared it.
                _destination.AdvanceStep();
                WaitingForInteraction = false;
                _resolvedIndoorPortalObjectId = 0;
                _resolvedIndoorPortalPosition = null;
                _activeInteractionObjectId = 0;
                _legIndex++;
                StartCurrentLeg();
                return;
            }
            if (IsIndoorPortalStep())
                _observedIndoorRoute = true;
            if (
                step.Kind == RouteStepKind.Travel
                && _destination.CurrentRoute is { Steps.Count: > 1 } route
                && route.Steps[1].Kind == RouteStepKind.Portal
                && TryResolveIndoorPortal(snapshot, step.To.Name)
            )
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
            if (
                step.Kind == RouteStepKind.Portal
                && (
                    TryResolveIndoorPortal(snapshot, step.Via) || _resolvedIndoorPortalObjectId != 0
                )
            )
            {
                _pausedForOutdoorRoute = false;
                _transitionGeneration++;
                TryStartInteraction(step);
                return;
            }
            if (NeedsDungeonExit(step) && TryResolveRouteDungeonExit(snapshot, step))
            {
                _indoorSurfaceExitLeg = true;
                _pausedForOutdoorRoute = false;
                StartIndoorWalk();
                if (!_isNavigating)
                {
                    _indoorSurfaceExitLeg = false;
                    _pausedForOutdoorRoute = true;
                }
                return;
            }
            if (NeedsDungeonExit(step))
            {
                _dungeonPortalSearch.UseRecordedExit(DungeonExitKey(step));
                DungeonTraversals.Observe(snapshot);
                string exit = RouteDungeonExitName(step) ?? $"exit near {step.From.Coords}";
                _isNavigating = false;
                _pausedForOutdoorRoute = true;
                if (_dungeonPortalSearch.TryGetNextTarget(_host.Automation, out var explorationTarget))
                {
                    var explorationStatus = _host.Automation.Navigation.GoTo(explorationTarget, 2.5f);
                    if (explorationStatus == PluginNavigationCommandStatus.Accepted)
                    {
                        _dungeonPortalSearch.TargetAccepted(explorationTarget.CellId);
                        _exploringDungeonExit = true;
                        _pausedForOutdoorRoute = false;
                        _isNavigating = true;
                        _activeSequence = _host.Automation.Navigation.GoToReport.Sequence;
                        _lastHandledReportRevision = 0;
                        _failureReason = $"Exploring dungeon for portal '{exit}'. {_dungeonPortalSearch.Reason}";
                        return;
                    }
                    _failureReason = $"Dungeon exploration request was {explorationStatus}.";
                    return;
                }
                _failureReason = IndoorPortalSearchReason(snapshot, step)
                    + " " + _dungeonPortalSearch.Reason;
                return;
            }
            _isNavigating = false;
            _pausedForOutdoorRoute = true;
            string reason = IndoorPortalSearchReason(snapshot, step);
            if (_failureReason != reason)
                _host.Log.Info($"GoArrow: {reason}");
            _failureReason = reason;
            return;
        }

        if (step.Kind != RouteStepKind.Travel)
        {
            _transitionGeneration++;
            TryStartInteraction(step);
            return;
        }

        var immediateTarget = step.To;
        _walkingPortalOrigin = _destination.CurrentRoute is { Steps.Count: > 1 } walkingRoute
            && walkingRoute.Steps[1].Kind == RouteStepKind.Portal ? snapshot.Position : null;
        _walkingPortalSawTransit = false;
        _outdoorPortalRetryObjectId = 0;
        _isNavigating = true;
        if (TryStartOutdoorPortalWalk())
            return;
        var navPos = new PluginNavigationPosition(
            CellId: 0,
            EastWest: immediateTarget.Coords.EW,
            NorthSouth: immediateTarget.Coords.NS,
            Elevation: double.NaN,
            HeadingDegrees: 0f,
            IsOutdoor: true
        );

        var status = _host.Automation.Navigation.GoTo(navPos, (float)_settings.ArrivalDistance);
        if (status != PluginNavigationCommandStatus.Accepted)
        {
            _isNavigating = false;
            _host.Log.Warn($"GoArrow: Navigation request was {status}.");
            return;
        }

        _activeSequence = _host.Automation.Navigation.GoToReport.Sequence;
        _lastHandledReportRevision = 0;
        _failureReason = string.Empty;
        _host.Log.Info(
            $"GoArrow: Navigating to {immediateTarget.Name} at ({navPos.EastWest:F2}, {navPos.NorthSouth:F2})"
        );
    }

    // IsOutdoor describes a cell, not whether the character took a portal.
    // A short walk into an envcell in the same landscape block retains the
    // outdoor route. Portal space, interaction, and discontinuous moves end it.
    private void UpdateLandscapeWalk(PluginNavigationSnapshot snapshot)
    {
        if (!snapshot.IsAvailable || snapshot.IsPortalSpace || WaitingForInteraction
            || _indoorWalk || _indoorRouteLeg || _indoorSurfaceExitLeg)
        {
            _landscapeWalkPosition = null;
            return;
        }
        if (snapshot.Position.IsOutdoor)
        {
            _landscapeWalkPosition = snapshot.Position;
            return;
        }
        if (_landscapeWalkPosition is { } previous
            && (previous.CellId >> 16) == (snapshot.Position.CellId >> 16)
            && previous.HorizontalDistanceMeters(snapshot.Position) <= 60)
            _landscapeWalkPosition = snapshot.Position;
        else
            _landscapeWalkPosition = null;
    }

    private string IndoorPortalSearchReason(PluginNavigationSnapshot snapshot, RouteStep step)
    {
        string expected = RouteDungeonExitName(step)
            ?? (NeedsDungeonExit(step) ? $"exit near {step.From.Coords} toward {step.To.Name}" : null)
            ?? (step.Kind == RouteStepKind.Portal ? step.Via : step.To.Name);
        var portals = _host.Automation.Objects.CaptureObjects()
            .Where(obj => obj.ObjectClass == PluginObjectClass.Portal
                || (obj.Capabilities & PluginObjectCapabilities.Portal) != 0)
            .OrderBy(obj => obj.ObjectId)
            .Take(8)
            .Select(obj => $"'{obj.Name}' destination='{obj.PortalDestination ?? (obj.HasAppraisalData ? "not provided" : "unknown")}' "
                + $"cell={obj.Position.CellId:X8} position={obj.HasPosition} "
                + $"outdoor={obj.Position.IsOutdoor} activate={obj.CanActivate}")
            .ToArray();
        return $"Waiting for indoor portal '{expected}'; player cell={snapshot.Position.CellId:X8}. "
            + $"Visible portals: {(portals.Length == 0 ? "none" : string.Join("; ", portals))}. "
            + $"Identification pending: {(_pendingPortalIdentificationObjectId == 0 ? "none" : $"{_pendingPortalIdentificationObjectId:X8}")}.";
    }

    private bool TryStartOutdoorPortalWalk(bool coordinateFailed = false)
    {
        var snapshot = _host.Automation.Navigation.Snapshot;
        if (_outdoorPortalRetryObjectId != 0 || !snapshot.IsAvailable
            || snapshot.IsPortalSpace
            || _destination.CurrentRoute is not { Steps.Count: > 1 } route
            || route.Steps[0].Kind != RouteStepKind.Travel
            || route.Steps[1].Kind != RouteStepKind.Portal)
            return false;
        var target = route.Steps[0].To;
        if (!snapshot.Position.IsOutdoor && !IsNearRouteEntrance(snapshot, target))
            return false;
        var portal = FindRouteEntrancePortal(target, snapshot, coordinateFailed);
        if (portal.ObjectId == 0)
            return false;
        _outdoorPortalRetryObjectId = portal.ObjectId;
        var status = _host.Automation.Navigation.GoTo(
            portal.ObjectId, (float)Math.Max(2.5, _settings.ArrivalDistance));
        if (status != PluginNavigationCommandStatus.Accepted)
            return false;
        _isNavigating = true;
        _observedIndoorRoute = false;
        _pausedForOutdoorRoute = false;
        _activeSequence = _host.Automation.Navigation.GoToReport.Sequence;
        _lastHandledReportRevision = 0;
        _failureReason = string.Empty;
        _host.Log.Info($"GoArrow: Approaching live portal '{portal.Name}' (0x{portal.ObjectId:X8}).");
        return true;
    }

    // A distant goal can fail because every currently reachable point is
    // farther from it. Try a bounded sideways/backward step, then let the
    // client plan the full route again from the reached position.
    private bool TryStartOutdoorDetour()
    {
        var snapshot = _host.Automation.Navigation.Snapshot;
        if (!snapshot.IsAvailable || snapshot.IsPortalSpace || !snapshot.Position.IsOutdoor
            || _destination.CurrentRoute is not { StepCount: > 0 } route
            || route.Steps[0].Kind != RouteStepKind.Travel)
            return false;
        var goal = route.Steps[0].To.Coords;
        if (!_outdoorDetourGoal.Equals(goal))
        {
            _outdoorDetourGoal = goal;
            _outdoorDetourAttempts = 0;
            _outdoorDetourTried.Clear();
        }
        if (_outdoorDetourAttempts >= 12)
            return false;
        var here = snapshot.Position;
        double dx = (goal.EW - here.EastWest) * 240;
        double dy = (goal.NS - here.NorthSouth) * 240;
        double length = Math.Sqrt(dx * dx + dy * dy);
        if (length < 100 || !double.IsFinite(length))
            return false;
        double forwardX = dx / length, forwardY = dy / length;
        double sideX = -forwardY, sideY = forwardX;
        // Sideways first; a retreat is used only if the nearby side routes
        // are unavailable. Alternating sides avoids a fixed turning bias.
        (double forward, double side)[] offsets =
            [(0, 80), (0, -80), (50, 80), (50, -80), (-50, 80), (-50, -80),
             (0, 150), (0, -150), (-80, 0), (80, 0)];
        var portals = _host.Automation.Objects.CaptureObjects()
            .Where(p => p.HasPosition && (p.ObjectClass == PluginObjectClass.Portal
                || (p.Capabilities & PluginObjectCapabilities.Portal) != 0))
            .Select(p => p.Position).ToArray();
        foreach (var offset in offsets)
        {
            if (_outdoorDetourAttempts >= 12)
                break;
            var candidate = new PluginNavigationPosition(0,
                here.EastWest + (forwardX * offset.forward + sideX * offset.side) / 240,
                here.NorthSouth + (forwardY * offset.forward + sideY * offset.side) / 240,
                double.NaN, 0, true);
            if (_outdoorDetourTried.Any(p => p.HorizontalDistanceMeters(candidate) < 25)
                || portals.Any(p => p.HorizontalDistanceMeters(candidate) < 20))
                continue;
            _outdoorDetourTried.Add(candidate);
            _outdoorDetourAttempts++;
            var status = _host.Automation.Navigation.GoTo(candidate, 4);
            if (status != PluginNavigationCommandStatus.Accepted)
                continue;
            _outdoorDetourActive = true;
            _isNavigating = true;
            _activeSequence = _host.Automation.Navigation.GoToReport.Sequence;
            _lastHandledReportRevision = 0;
            _failureReason = $"Trying outdoor detour {_outdoorDetourAttempts}/12 toward "
                + $"{candidate.NorthSouth:0.00}, {candidate.EastWest:0.00}.";
            return true;
        }
        return false;
    }

    private static bool IsNearRouteEntrance(PluginNavigationSnapshot snapshot, Location target) =>
        new Coordinates(snapshot.Position.NorthSouth, snapshot.Position.EastWest)
            .DistanceTo(target.Coords) * 240 <= 60;

    private PluginWorldObject FindRouteEntrancePortal(
        Location target, PluginNavigationSnapshot snapshot, bool coordinateFailed)
    {
        string expected = target.Name.StartsWith("Town Network Portal(", StringComparison.OrdinalIgnoreCase)
            ? "Town Network" : PortalDestinationName(target.Name);
        return _host.Automation.Objects.CaptureObjects()
            .Where(obj => obj.HasPosition && obj.CanActivate
                && !IsUnknownSurfacePortal(obj)
                && (obj.Capabilities & PluginObjectCapabilities.Portal) != 0
                && (coordinateFailed || snapshot.Position.HorizontalDistanceMeters(obj.Position) <= 60))
            .Select(obj => new
            {
                Object = obj,
                Distance = target.Coords.DistanceTo(new Coordinates(obj.Position.NorthSouth, obj.Position.EastWest)),
                Matches = PortalMatchesDestination(obj, expected),
            })
            .Where(candidate => candidate.Distance <= (candidate.Matches ? 0.25 : 0.1))
            .OrderBy(candidate => candidate.Matches ? 0 : 1)
            .ThenBy(candidate => candidate.Distance)
            .Select(candidate => candidate.Object)
            .FirstOrDefault();
    }

    private bool IsIndoorPortalStep()
    {
        if (_destination.CurrentRoute is not { StepCount: > 0 } route)
            return false;
        return route.Steps[0].Kind == RouteStepKind.Portal
            || (
                route.Steps[0].Kind == RouteStepKind.Travel
                && route.StepCount > 1
                && route.Steps[1].Kind == RouteStepKind.Portal
            );
    }

    private bool IsNearTownNetworkExit(PluginNavigationPosition position)
    {
        if (
            _destination.CurrentRoute is not { StepCount: > 1 } route
            || route.Steps[0].Kind != RouteStepKind.Travel
            || route.Steps[1].Kind != RouteStepKind.Portal
            || !route
                .Steps[0]
                .To.Name.StartsWith("Town Network ", StringComparison.OrdinalIgnoreCase)
            || !route.Steps[1].To.HasCoordinates
        )
            return false;

        var here = new Coordinates(position.NorthSouth, position.EastWest);
        return here.DistanceTo(route.Steps[1].To.Coords) * 240 <= 250;
    }

    private bool CanNavigateIndoorTarget(PluginNavigationSnapshot snapshot)
    {
        PluginNavigationPosition? targetPosition =
            _destination.TargetIndoorPosition ?? _resolvedIndoorPortalPosition;
        if (
            _destination.Kind == GoArrowDestinationKind.Object
            && !_destination.TargetUnavailable
            && _destination.TargetObjectId is > 0
        )
            targetPosition = _destination.TargetObjectPosition;
        bool liveObjectTarget =
            _resolvedIndoorPortalObjectId != 0
            || (
                _destination.Kind == GoArrowDestinationKind.Object
                && !_destination.TargetUnavailable
                && _destination.TargetObjectId is > 0
            );
        return snapshot.IsAvailable
            && !snapshot.IsPortalSpace
            && !snapshot.Position.IsOutdoor
            && targetPosition is { IsOutdoor: false } target
            && snapshot.Position.CellId != 0
            && target.CellId != 0
            && (
                liveObjectTarget
                    ? AreNearbyIndoorLandblocks(snapshot.Position.CellId, target.CellId)
                    : (snapshot.Position.CellId & 0xFFFF0000u) == (target.CellId & 0xFFFF0000u)
            );
    }

    /// <summary>Finds the same indoor target that a direct walk would use.</summary>
    internal bool TryGetIndoorPreviewTarget(out uint objectId, out PluginNavigationPosition position)
    {
        objectId = 0;
        position = default;
        var snapshot = _host.Automation.Navigation.Snapshot;
        if (!snapshot.IsAvailable || snapshot.IsPortalSpace || snapshot.Position.IsOutdoor)
            return false;

        if (
            _destination.Kind == GoArrowDestinationKind.Object
            && CanNavigateIndoorTarget(snapshot)
            && _destination.TargetObjectId is { } selectedId
            && _destination.TargetObjectPosition is { } selectedPosition
        )
        {
            objectId = selectedId;
            position = selectedPosition;
            return true;
        }

        if (
            _destination.TargetIndoorPosition is { } indoorPosition
            && CanNavigateIndoorTarget(snapshot)
        )
        {
            position = indoorPosition;
            return true;
        }

        if (
            _resolvedIndoorPortalObjectId != 0
            && CanNavigateIndoorTarget(snapshot)
            && _resolvedIndoorPortalPosition is { } portalPosition
        )
        {
            objectId = _resolvedIndoorPortalObjectId;
            position = portalPosition;
            return true;
        }

        if (
            _destination.Kind == GoArrowDestinationKind.Location
            && _destination.TargetIndoorPosition is null
        )
        {
            PluginWorldObject portal = FindIndoorPortal(snapshot, _destination.TargetName);
            if (portal.ObjectId != 0)
            {
                objectId = portal.ObjectId;
                position = portal.Position;
                return true;
            }
        }

        return false;
    }

    private bool TryResolveIndoorPortal(PluginNavigationSnapshot snapshot)
    {
        if (
            !snapshot.IsAvailable
            || snapshot.IsPortalSpace
            || snapshot.Position.IsOutdoor
            || snapshot.Position.CellId == 0
            || _destination.Kind != GoArrowDestinationKind.Location
            || _destination.TargetIndoorPosition is not null
        )
            return false;

        return TryResolveIndoorPortal(snapshot, _destination.TargetName);
    }

    private bool TryResolveIndoorPortal(PluginNavigationSnapshot snapshot, string name)
    {
        PluginWorldObject portal = FindIndoorPortal(snapshot, name);
        if (portal.ObjectId == 0)
            return false;
        _resolvedIndoorPortalObjectId = portal.ObjectId;
        _resolvedIndoorPortalPosition = portal.Position;
        return true;
    }

    private PluginWorldObject FindIndoorPortal(PluginNavigationSnapshot snapshot, string name)
    {
        if (
            !snapshot.IsAvailable
            || snapshot.IsPortalSpace
            || snapshot.Position.IsOutdoor
            || snapshot.Position.CellId == 0
        )
            return default;

        string destinationName = PortalDestinationName(name);
        if (destinationName.Length == 0)
            return default;
        return _host
            .Automation.Objects.CaptureObjects()
            .Where(obj =>
                obj.ObjectId != 0
                && obj.HasPosition
                && obj.CanActivate
                && !IsUnknownSurfacePortal(obj)
                && (obj.Capabilities & PluginObjectCapabilities.Portal) != 0
                && !obj.Position.IsOutdoor
                && obj.Position.CellId != 0
                && AreNearbyIndoorLandblocks(snapshot.Position.CellId, obj.Position.CellId)
                && PortalMatchesDestination(obj, destinationName)
            )
            .OrderBy(obj => obj.PortalDestination is null ? 1 : 0)
            .ThenBy(obj => snapshot.Position.HorizontalDistanceMeters(obj.Position))
            .FirstOrDefault();
    }

    private static string? RouteDungeonExitName(RouteStep step)
    {
        if (step.Kind != RouteStepKind.Travel)
            return null;
        string origin = step.From.Name;
        int arrival = origin.LastIndexOf(" arrival (", StringComparison.OrdinalIgnoreCase);
        if (arrival < 0)
            return null;
        origin = origin[..arrival];
        int destination = origin.LastIndexOf(" to ", StringComparison.OrdinalIgnoreCase);
        if (destination < 0)
            return null;
        string name = PortalDestinationName(origin[(destination + 4)..]);
        return name.Length > 0 ? name : null;
    }

    private bool NeedsDungeonExit(RouteStep step) => step.Kind == RouteStepKind.Travel
        && step.From.Name.Contains(" arrival (", StringComparison.OrdinalIgnoreCase)
        && (RouteDungeonExitName(step) is not null
            || _destination.CurrentRoute is not { Steps.Count: > 1 } route
            || route.Steps[1].Kind != RouteStepKind.Portal);

    private bool TryResolveRouteDungeonExit(PluginNavigationSnapshot snapshot, RouteStep step)
    {
        var portals = _host.Automation.Objects.CaptureObjects()
            .Where(p => p.HasPosition && p.CanActivate && !p.Position.IsOutdoor
                && (p.Capabilities & PluginObjectCapabilities.Portal) != 0
                && AreNearbyIndoorLandblocks(snapshot.Position.CellId, p.Position.CellId)).ToArray();
        string? exitName = RouteDungeonExitName(step);
        RequestPortalDestination(portals, exitName);
        var matches = portals.Where(p =>
        {
            if (string.IsNullOrWhiteSpace(p.PortalDestination))
                return false;
            // Arrival labels often contain rounded outdoor coordinates. A
            // reported coordinate takes precedence over an object's name.
            if (Coordinates.TryParse(p.PortalDestination, out var arrival))
                return arrival.DistanceTo(step.From.Coords) * 240 <= 30;
            return PortalMatchesDestination(p, exitName ?? PortalDestinationName(step.To.Name));
        }).Take(2).ToArray();
        if (matches.Length != 1)
            return false;
        _resolvedIndoorPortalObjectId = matches[0].ObjectId;
        _resolvedIndoorPortalPosition = matches[0].Position;
        DungeonTraversals.RememberExit(DungeonExitKey(step), matches[0], snapshot);
        return true;
    }

    private static string DungeonExitKey(RouteStep step) => FormattableString.Invariant(
        $"{RouteDungeonExitName(step)?.ToUpperInvariant()}|{step.From.Coords.NS:R}|{step.From.Coords.EW:R}");

    private bool TryResolveSurfaceExit(
        PluginNavigationSnapshot snapshot,
        string? expectedDestination = null
    )
    {
        if (snapshot.Position.CellId == 0)
            return false;
        // Atlas routes can place an outdoor portal at the same map coordinates
        // as a dungeon exit. The live exit is often just named "Surface Portal".
        // Prefer a matching destination. A unique Surface Portal is eligible
        // only after identification has supplied its destination.
        var portals = _host.Automation.Objects.CaptureObjects()
            .Where(obj =>
                obj.ObjectId != 0
                && obj.HasPosition
                && obj.CanActivate
                && (obj.Capabilities & PluginObjectCapabilities.Portal) != 0
                && !obj.Position.IsOutdoor
                && obj.Position.CellId != 0
                && AreNearbyIndoorLandblocks(snapshot.Position.CellId, obj.Position.CellId)
            )
            .ToArray();
        PluginWorldObject chosen = default;
        if (!string.IsNullOrWhiteSpace(expectedDestination))
        {
            string destination = PortalDestinationName(expectedDestination);
            var matching = portals.Where(obj =>
                obj.PortalDestination is { } label
                && PortalDestinationName(label).Equals(
                    destination,
                    StringComparison.OrdinalIgnoreCase
                )
            ).Take(2).ToArray();
            if (matching.Length == 1)
                chosen = matching[0];
            else if (matching.Length > 1)
                return false;
        }
        if (chosen.ObjectId == 0)
        {
            var surfaces = portals.Where(obj =>
                obj.Name.Equals("Surface Portal", StringComparison.OrdinalIgnoreCase)
            ).Take(2).ToArray();
            RequestPortalDestination(surfaces);
            if (surfaces.Length == 1 && !string.IsNullOrWhiteSpace(surfaces[0].PortalDestination))
                chosen = surfaces[0];
        }
        if (chosen.ObjectId == 0)
            return false;
        _resolvedIndoorPortalObjectId = chosen.ObjectId;
        _resolvedIndoorPortalPosition = chosen.Position;
        return true;
    }

    private static bool PortalMatchesDestination(PluginWorldObject portal, string destination)
    {
        // Appraisal destinations are display labels and can include coordinates
        // or other text. They must not suppress a matching live portal name.
        return PortalDestinationName(portal.Name).Equals(
                destination,
                StringComparison.OrdinalIgnoreCase
            )
            || (portal.PortalDestination is { } known
                && PortalDestinationName(known).Equals(
                destination,
                StringComparison.OrdinalIgnoreCase
            ));
    }

    private static bool IsUnknownSurfacePortal(PluginWorldObject portal) =>
        portal.Name.Equals("Surface Portal", StringComparison.OrdinalIgnoreCase)
        && string.IsNullOrWhiteSpace(portal.PortalDestination);

    private bool EnsurePortalDestinationKnown(uint objectId)
    {
        if (!_host.Automation.Objects.TryGet(objectId, out var portal)
            || !portal.Name.Equals("Surface Portal", StringComparison.OrdinalIgnoreCase)
            || !string.IsNullOrWhiteSpace(portal.PortalDestination))
            return true;
        RequestPortalDestination([portal]);
        _waitingForSurfaceExit = false;
        WaitingForInteraction = false;
        _pausedForOutdoorRoute = true;
        _failureReason = "Waiting for Surface Portal identification; destination is unknown.";
        return false;
    }

    private void RequestPortalDestination(PluginWorldObject[] portals, string? preferredDestination = null)
    {
        if (_pendingPortalIdentificationObjectId != 0)
        {
            // A missing appraisal response must not block every later portal.
            // A late response still updates the object and can be used on the next tick.
            if (Stopwatch.GetElapsedTime(_pendingPortalIdentificationStarted).TotalSeconds < 5)
                return;
            _pendingPortalIdentificationObjectId = 0;
            _pendingPortalIdentificationStarted = 0;
        }
        foreach (PluginWorldObject portal in portals
            .OrderByDescending(p => preferredDestination is not null
                && PortalDestinationName(p.Name).Equals(preferredDestination,
                    StringComparison.OrdinalIgnoreCase)))
        {
            if (!string.IsNullOrWhiteSpace(portal.PortalDestination)
                || portal.HasAppraisalData
                || _portalIdentificationAttempted.Contains(portal.ObjectId))
                continue;
            if (_host.Automation.Objects.Identify(portal.ObjectId).Accepted)
            {
                _portalIdentificationAttempted.Add(portal.ObjectId);
                _pendingPortalIdentificationObjectId = portal.ObjectId;
                _pendingPortalIdentificationStarted = Stopwatch.GetTimestamp();
            }
            return;
        }
    }

    private static bool AreNearbyIndoorLandblocks(uint first, uint second)
    {
        int firstX = (int)(first >> 24);
        int firstY = (int)((first >> 16) & 0xFFu);
        int secondX = (int)(second >> 24);
        int secondY = (int)((second >> 16) & 0xFFu);
        return Math.Abs(firstX - secondX) <= 2 && Math.Abs(firstY - secondY) <= 2;
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
        else if (
            value.StartsWith("Town Network ", StringComparison.OrdinalIgnoreCase)
            && value.LastIndexOf(" to ", StringComparison.OrdinalIgnoreCase) is var toIndex and >= 0
        )
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
        if (
            _destination.Kind == GoArrowDestinationKind.Object
            && _destination.TargetObjectId is { } objectId
        )
            status = _host.Automation.Navigation.GoTo(objectId, (float)_settings.ArrivalDistance);
        else if (_resolvedIndoorPortalObjectId != 0)
            status = _host.Automation.Navigation.GoTo(
                _resolvedIndoorPortalObjectId,
                (float)_settings.ArrivalDistance
            );
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
        if (
            report.Owner is not null
            && !report.Owner.Equals(PluginOwner, StringComparison.OrdinalIgnoreCase)
        )
            return;
        if (_activeSequence != 0 && report.Sequence != 0 && report.Sequence != _activeSequence)
            return;

        if (report.Revision != 0 && report.Revision == _lastHandledReportRevision)
            return;

        if (report.State == PluginGoToState.Lost && _walkingPortalOrigin is not null
            && (_host.Automation.Navigation.Snapshot.IsPortalSpace
                || report.Reason?.Contains("portal space", StringComparison.OrdinalIgnoreCase) == true))
        {
            _walkingPortalSawTransit = true;
            _lastHandledReportRevision = report.Revision;
            PauseForPortalExit();
            TryCompleteWalkingPortal(_host.Automation.Navigation.Snapshot);
            return;
        }

        if (_exploringDungeonExit)
        {
            if (report.State is PluginGoToState.Arrived or PluginGoToState.ArrivedWithoutSight
                or PluginGoToState.NoRoute or PluginGoToState.Lost)
            {
                _lastHandledReportRevision = report.Revision;
                bool arrived = report.State is PluginGoToState.Arrived or PluginGoToState.ArrivedWithoutSight;
                _dungeonPortalSearch.CompleteTarget(_host.Automation.Navigation.Snapshot.Position, arrived);
                if (!arrived)
                    _dungeonPortalSearch.RecordFailure(report.Reason);
                _exploringDungeonExit = false;
                _isNavigating = false;
                _pausedForOutdoorRoute = true;
                _indoorRetryElapsed = 0;
            }
            else if (report.State is PluginGoToState.Blocked or PluginGoToState.Interrupted
                or PluginGoToState.Stopped)
            {
                StopNavigation();
                _failureReason = report.Reason ?? report.State.ToString();
            }
            return;
        }

        if (_indoorWalk && report.State == PluginGoToState.ArrivedWithoutSight)
        {
            _lastHandledReportRevision = report.Revision;
            _isNavigating = false;
            _indoorWalk = false;
            _failureReason = report.Reason ?? "Indoor destination was not reachable.";
            _host.Automation.Chat.PostSystemMessage($"GoArrow: {_failureReason}");
            return;
        }

        if (report.State == PluginGoToState.ArrivedWithoutSight)
        {
            _lastHandledReportRevision = report.Revision;
            _isNavigating = false;
            string target =
                _destination.CurrentRoute?.Steps.FirstOrDefault()?.To.Name
                ?? _destination.TargetName;
            string distance =
                float.IsFinite(report.RemainingMeters) && report.RemainingMeters > 0
                    ? $" ({report.RemainingMeters:0} m remaining)"
                    : string.Empty;
            _failureReason =
                $"Stopped short of '{target}'{distance}."
                + (string.IsNullOrWhiteSpace(report.Reason) ? string.Empty : $" {report.Reason}");
            _host.Log.Warn($"GoArrow: {_failureReason}");
            _host.Automation.Chat.PostSystemMessage($"GoArrow: {_failureReason}");
            return;
        }

        if (report.State == PluginGoToState.Arrived)
        {
            _lastHandledReportRevision = report.Revision;
            if (_outdoorDetourActive)
            {
                _outdoorDetourActive = false;
                _isNavigating = false;
                var snapshot = _host.Automation.Navigation.Snapshot;
                if (snapshot.IsAvailable && !snapshot.IsPortalSpace && snapshot.Position.IsOutdoor
                    && PlanRoute())
                {
                    StartCurrentLeg();
                    return;
                }
                _failureReason = "Outdoor detour ended without a usable outdoor route.";
                return;
            }
            if (_indoorWalk)
            {
                _isNavigating = false;
                _indoorWalk = false;
                if (_indoorSurfaceExitLeg)
                {
                    _indoorSurfaceExitLeg = false;
                    StartSurfaceExitInteraction();
                    return;
                }
                if (_indoorRouteLeg)
                {
                    _indoorRouteLeg = false;
                    HandleWaypointReached();
                    return;
                }
                HasArrived = true;
                _activeSequence = 0;
                _host.Automation.Chat.PostSystemMessage(
                    $"GoArrow: Arrived at '{_destination.TargetName}'."
                );
                return;
            }
            HandleWaypointReached();
            return;
        }

        if (
            report.State
            is PluginGoToState.NoRoute
                or PluginGoToState.Blocked
                or PluginGoToState.Interrupted
                or PluginGoToState.Lost
        )
        {
            if (report.State == PluginGoToState.NoRoute && !_indoorWalk && TryStartOutdoorPortalWalk(coordinateFailed: true))
                return;
            if (report.State == PluginGoToState.NoRoute && !_indoorWalk
                && (_outdoorDetourActive || (report.Reason?.Contains("no clear path leads any nearer the goal",
                    StringComparison.OrdinalIgnoreCase) ?? false))
                && TryStartOutdoorDetour())
                return;
            bool detourExhausted = _outdoorDetourActive && report.State == PluginGoToState.NoRoute;
            _outdoorDetourActive = false;
            _lastHandledReportRevision = report.Revision;
            if (
                report.State == PluginGoToState.Lost
                && (_indoorRouteLeg || _indoorSurfaceExitLeg)
                && _host.Automation.Navigation.Snapshot.IsPortalSpace
            )
            {
                PauseForPortalExit();
                return;
            }
            if (
                !_indoorWalk
                && report.State == PluginGoToState.Blocked
                && report.BlockedByObjectId != 0
            )
            {
                if (
                    _host.Automation.Objects.TryGet(
                        report.BlockedByObjectId,
                        out PluginWorldObject blocked
                    )
                    && blocked.ObjectClass == PluginObjectClass.Door
                    && !blocked.IsDoorOpen
                    && blocked.CanActivate
                )
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
            _failureReason = (report.Reason ?? report.State.ToString())
                + (detourExhausted ? $" after {_outdoorDetourAttempts} outdoor detour attempts." : "");
            _host.Log.Warn(
                $"GoArrow: Navigation stopped with state {report.State}: {_failureReason}."
            );
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Navigation stopped ({report.State})."
            );
        }
    }

    private void PauseForPortalExit()
    {
        _isNavigating = false;
        _indoorWalk = false;
        _indoorRouteLeg = false;
        _pausedForOutdoorRoute = true;
        _activeSequence = 0;
        _failureReason = "Waiting for portal transition.";
    }

    private void StartSurfaceExitInteraction()
    {
        _activeInteractionObjectId = _resolvedIndoorPortalObjectId;
        if (!EnsurePortalDestinationKnown(_activeInteractionObjectId))
            return;
        _waitingForSurfaceExit = true;
        _pausedForOutdoorRoute = true;
        WaitingForInteraction = true;
        _failureReason = "Waiting for surface portal transition.";
        var result = _host.Automation.Objects.Activate(_activeInteractionObjectId);
        if (!result.Accepted)
        {
            _waitingForSurfaceExit = false;
            _pausedForOutdoorRoute = false;
            WaitingForInteraction = false;
            _failureReason = $"Surface Portal activation was not accepted ({result.Status}).";
            _host.Automation.Chat.PostSystemMessage($"GoArrow: {_failureReason}");
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
        _walkingPortalOrigin = null;
        _walkingPortalSawTransit = false;
        _destination.AdvanceStep();
        _legIndex++;
        if (_destination.CurrentRoute == null || _destination.CurrentRoute.StepCount == 0)
        {
            // Arrived at final destination
            HasArrived = true;
            _isNavigating = false;
            _host.Automation.Navigation.StopGoTo();
            _activeSequence = 0;
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Arrived at '{_destination.TargetName}'."
            );
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
        _activeInteractionObjectId =
            step.ObjectId != 0 ? step.ObjectId
            : step.Kind == RouteStepKind.Portal && _outdoorPortalRetryObjectId != 0 ? _outdoorPortalRetryObjectId
            : step.Kind == RouteStepKind.Portal ? _resolvedIndoorPortalObjectId
            : 0;
        if (_activeInteractionObjectId == 0)
        {
            PluginObjectCapabilities required =
                step.Kind == RouteStepKind.Portal
                    ? PluginObjectCapabilities.Portal
                    : PluginObjectCapabilities.Interactable;
            var candidate = _host
                .Automation.Objects.CaptureObjects()
                .Where(obj => obj.CanActivate && (obj.Capabilities & required) != 0)
                .Select(obj => new
                {
                    Object = obj,
                    Distance = obj.HasPosition
                        ? new RouteFinding.Coordinates(
                            obj.Position.NorthSouth,
                            obj.Position.EastWest
                        ).DistanceTo(step.From.Coords)
                        : double.PositiveInfinity,
                    NameMatches = !string.IsNullOrWhiteSpace(step.Via)
                        && obj.Name.Contains(step.Via, StringComparison.OrdinalIgnoreCase),
                })
                .Where(candidate =>
                    candidate.NameMatches
                        ? !candidate.Object.HasPosition || candidate.Distance <= 1.0
                        : step.Kind == RouteStepKind.Portal && candidate.Distance <= 0.1
                )
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
                $"GoArrow: Could not identify {step.Kind.ToString().ToLowerInvariant()} '{step.Via}'. Complete it manually, then use /go resume."
            );
            return;
        }

        if (!EnsurePortalDestinationKnown(_activeInteractionObjectId))
            return;
        WaitingForInteraction = true;
        var result = _host.Automation.Objects.Activate(_activeInteractionObjectId);
        if (!result.Accepted)
        {
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Interaction with '{step.Via}' was not accepted ({result.Status}); use /go resume if completed manually."
            );
            return;
        }
        _host.Automation.Chat.PostSystemMessage($"GoArrow: Activating '{step.Via}'.");
    }

    private void OnActivationCompleted(PluginActivationCompletion completion)
    {
        if (
            !WaitingForInteraction
            || completion.ObjectId != _activeInteractionObjectId
            || completion.Revision == 0
            || completion.Revision <= _lastActivationRevision
        )
            return;
        _lastActivationRevision = completion.Revision;
        if (_waitingForSurfaceExit)
        {
            if (!completion.IsSuccess)
            {
                _waitingForSurfaceExit = false;
                _pausedForOutdoorRoute = false;
                WaitingForInteraction = false;
                _failureReason = $"Surface Portal activation failed ({completion.Outcome}).";
                _host.Automation.Chat.PostSystemMessage($"GoArrow: {_failureReason}");
            }
            return;
        }
        if (!completion.IsSuccess)
        {
            _host.Log.Warn(
                $"GoArrow: Interaction failed ({completion.Outcome}) for object 0x{completion.ObjectId:X8}."
            );
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Interaction failed ({completion.Outcome}); use /go resume to retry."
            );
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
        if (change.Kind == PluginObjectChangeKind.Released)
        {
            _portalIdentificationAttempted.Remove(change.ObjectId);
            if (change.ObjectId == _pendingPortalIdentificationObjectId)
            {
                _pendingPortalIdentificationObjectId = 0;
                _pendingPortalIdentificationStarted = 0;
            }
        }
        if (
            change.Kind == PluginObjectChangeKind.IdentReceived
            && change.ObjectId == _pendingPortalIdentificationObjectId
        )
        {
            _pendingPortalIdentificationObjectId = 0;
            _pendingPortalIdentificationStarted = 0;
            if (_pausedForOutdoorRoute && _destination.CurrentRoute is { StepCount: > 0 })
                StartCurrentLeg();
            else if (!_isNavigating && !WaitingForInteraction && _destination.HasDestination)
                StartNavigation();
            return;
        }
        if (
            _destination.Kind != GoArrowDestinationKind.Object
            || _destination.TargetObjectId != change.ObjectId
        )
            return;
        if (change.Kind == PluginObjectChangeKind.Released || change.Current is not { } current)
        {
            _destination.MarkObjectUnavailable();
            return;
        }
        _destination.UpdateObject(current);
    }

    private bool TryCompleteWalkingPortal(PluginNavigationSnapshot snapshot)
    {
        if (!_walkingPortalSawTransit || _walkingPortalOrigin is not { } origin
            || !snapshot.IsAvailable || snapshot.IsPortalSpace
            || _destination.CurrentRoute is not { Steps.Count: > 1 } route
            || route.Steps[0].Kind != RouteStepKind.Travel
            || route.Steps[1].Kind != RouteStepKind.Portal)
            return false;
        bool changedWorld = snapshot.Position.IsOutdoor != origin.IsOutdoor
            || (origin.CellId != 0 && snapshot.Position.CellId != 0
                && (origin.CellId >> 16) != (snapshot.Position.CellId >> 16));
        if (!changedWorld && snapshot.Position.HorizontalDistanceMeters(origin) <= 20)
            return false;
        // Walking into a portal can teleport before GoTo reports arrival.
        // Complete the approach and crossing together, without activating again.
        _walkingPortalOrigin = null;
        _walkingPortalSawTransit = false;
        _destination.AdvanceStep();
        _destination.AdvanceStep();
        _legIndex += 2;
        _isNavigating = false;
        _pausedForOutdoorRoute = false;
        WaitingForInteraction = false;
        _activeSequence = 0;
        _outdoorPortalRetryObjectId = 0;
        _resolvedIndoorPortalObjectId = 0;
        _resolvedIndoorPortalPosition = null;
        _landscapeWalkPosition = null;
        _failureReason = string.Empty;
        if (_destination.CurrentRoute?.StepCount == 0)
            HasArrived = true;
        else
            StartCurrentLeg();
        return true;
    }

    private void OnPortalTransition(PluginPortalTransition transition)
    {
        DungeonTraversals.BreakTrace();
        if (_walkingPortalOrigin is not null && transition.IsCompleted
            && transition.Kind != PluginPortalTransitionKind.Login)
        {
            _walkingPortalSawTransit = true;
            if (TryCompleteWalkingPortal(_host.Automation.Navigation.Snapshot))
                return;
        }
        if (
            !WaitingForInteraction
            || !transition.IsCompleted
            || transition.Kind == PluginPortalTransitionKind.Login
            || _destination.CurrentRoute is not { StepCount: > 0 } route
            || route.Steps[0].Kind is not (RouteStepKind.Portal or RouteStepKind.Recall)
        )
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
        if (
            !WaitingForInteraction
            || _destination.CurrentRoute is not { StepCount: > 0 } route
            || route.Steps[0].Kind is not (RouteStepKind.Portal or RouteStepKind.Recall)
            || _interactionOriginPosition is not { } origin
        )
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
        bool changedWorld =
            current.IsOutdoor != origin.IsOutdoor
            || (
                current.CellId != 0
                && origin.CellId != 0
                && (current.CellId & 0xFFFF0000u) != (origin.CellId & 0xFFFF0000u)
            );
        bool movedAfterPortalSpace =
            _interactionSawPortalSpace
            && (current.CellId != origin.CellId || current.HorizontalDistanceMeters(origin) > 20);
        if (!changedWorld && !movedAfterPortalSpace)
            return;

        _host.Log.Info(
            $"GoArrow: Completed '{route.Steps[0].Via}' from the live position after portal transition."
        );
        ResumeAfterInteraction();
    }

    public void Dispose()
    {
        Disable();
        StopNavigation();
    }
}
