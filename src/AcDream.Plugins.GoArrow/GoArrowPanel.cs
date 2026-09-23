using AcDream.Plugin.Abstractions;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow;

/// <summary>
/// Data-binding surface for the GoArrow plugin's declarative panel.
/// Properties and actions are consumed by goarrow-panel.xml markup.
/// </summary>
internal sealed class GoArrowPanel
{
    private const string CurrentLocation = "Current Location";
    private readonly IPluginHost _host;
    private readonly GoArrowPlugin _plugin;
    private readonly GoArrowSettings _settings;
    private readonly GoArrowDestination _destination;
    private readonly GoArrowNavigator _navigator;
    private bool _searchingFrom;
    private bool _showSearchResults;
    private bool _editingFrom;
    private bool _editingDestination = true;
    private bool _showConfig;
    private bool _showDetails;
    private Route? _selectedRoute;
    private int _selectedRouteStep;
    private Location? _notesLocation;
    private IReadOnlyList<string> _notesLines = [];
    private string _locationUrlStatus = string.Empty;
    private string _dungeonUrlStatus = string.Empty;

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
        DestinationInput = settings.DestinationName;
        LocationDataUrlInput = settings.ExternalDataUrl;
        DungeonMapUrlInput = settings.DungeonMapUrl;
    }

    // ── Panel display properties ────────────────────────────────────

    public string TitleText => GoArrowPlugin.DisplayTitle;
    public Action ResetOverlayPositionsAction => _plugin.ResetOverlayPositions;

    /// <summary>The current destination name.</summary>
    public string DestinationText =>
        string.IsNullOrEmpty(_destination.TargetName)
            ? "[None]"
            : _destination.TargetName;

    /// <summary>Distance to destination (formatted).</summary>
    public string DistanceText =>
        _destination.HasDestination && _settings.ShowDistance && !OutdoorRoutePaused
            ? TravelDistance.Format(_destination.EstimatedDistance)
            : string.Empty;

    /// <summary>Bearing to the next route waypoint (formatted).</summary>
    public string BearingText =>
        _destination.HasDestination && _settings.ShowBearing && !OutdoorRoutePaused
            ? $"{_destination.BearingDegrees:F1}°"
            : string.Empty;

    /// <summary>Navigation status text.</summary>
    public string NavStatusText
    {
        get
        {
            if (_plugin.IsComputingRoute)
                return "Computing route...";
            if (_navigator.IsPlanningPath)
                return "Client planning path...";
            if (_navigator.IsNavigating)
                return "Navigating...";
            if (_navigator.WaitingForInteraction)
                return "Waiting for interaction";
            if (_navigator.HasArrived)
                return "Arrived!";
            if (string.IsNullOrEmpty(_destination.TargetName))
                return "Idle";
            if (_navigator.WaitingForIndoorPortal)
                return "Finding indoor portal";
            if (_navigator.HasIndoorTarget)
                return "Indoor target ready";
            if (OutdoorRoutePaused)
                return "Outdoor route paused indoors";
            return _destination.CurrentRoute is { StepCount: > 0 } ? "Route ready" : "Ready";
        }
    }

    private bool OutdoorRoutePaused
    {
        get
        {
            var snapshot = _host.Automation.Navigation.Snapshot;
            return snapshot.IsAvailable && (snapshot.IsPortalSpace || !snapshot.Position.IsOutdoor);
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
    public bool IsNavigating => _navigator.IsNavigating || _plugin.IsComputingRoute;

    /// <summary>Whether the route is paused for a manual portal/recall action.</summary>
    public bool WaitingForInteraction => _navigator.WaitingForInteraction;

    /// <summary>Editable destination input used by panel hosts that support text controls.</summary>
    public string DestinationInput { get; set; } = string.Empty;

    public string FromInput { get; set; } = CurrentLocation;

    public string FromEditorInput { get; private set; } = string.Empty;

    public string FromText { get; private set; } = CurrentLocation;

    public bool FromEditorVisible => _editingFrom;

    public bool FromSelectionVisible => !_editingFrom;

    public bool DestinationEditorVisible => _editingDestination;

    public bool DestinationSelectionVisible => !_editingDestination;

    public bool RouteTabSelected => !_showConfig && !_showDetails;
    public bool ConfigTabSelected => _showConfig;
    public bool DetailsTabSelected => _showDetails;
    public bool RouteTabVisible => !_showConfig && !_showDetails;
    public bool ConfigTabVisible => _showConfig;
    public bool DetailsTabVisible => _showDetails;
    public Action ShowRouteTab => () =>
    {
        _showConfig = false;
        _showDetails = false;
    };
    public Action ShowConfigTab => () =>
    {
        _showSearchResults = false;
        _showConfig = true;
        _showDetails = false;
    };
    public Action ShowDetailsTab => () =>
    {
        _showSearchResults = false;
        _showConfig = false;
        _showDetails = true;
    };

    public string LocationDataUrlInput { get; set; }
    public string DungeonMapUrlInput { get; set; }
    public string LocationDownloadStatus => _locationUrlStatus.Length > 0
        ? _locationUrlStatus : _plugin.LocationDownloadStatus;
    public string DungeonDownloadStatus => _dungeonUrlStatus.Length > 0
        ? _dungeonUrlStatus : _plugin.DungeonDownloadStatus;

    public Action<string> UpdateLocationDataUrlAction => value =>
    {
        LocationDataUrlInput = value;
        _locationUrlStatus = string.Empty;
    };
    public Action<string> SubmitLocationDataUrlAction => value =>
    {
        LocationDataUrlInput = value;
        SaveLocationDataUrl();
    };
    public Action SaveLocationDataUrlAction => () => SaveLocationDataUrl();
    public Action DownloadLocationDataAction => () =>
    {
        if (SaveLocationDataUrl())
        {
            _locationUrlStatus = string.Empty;
            _ = _plugin.UpdateDataAsync();
        }
    };

    public Action<string> UpdateDungeonMapUrlAction => value =>
    {
        DungeonMapUrlInput = value;
        _dungeonUrlStatus = string.Empty;
    };
    public Action<string> SubmitDungeonMapUrlAction => value =>
    {
        DungeonMapUrlInput = value;
        SaveDungeonMapUrl();
    };
    public Action SaveDungeonMapUrlAction => () => SaveDungeonMapUrl();
    public Action DownloadDungeonMapsAction => () =>
    {
        if (SaveDungeonMapUrl())
        {
            _dungeonUrlStatus = string.Empty;
            _ = _plugin.UpdateDungeonMapsAsync();
        }
    };

    private bool SaveLocationDataUrl()
    {
        if (!_plugin.SetExternalDataUrl(LocationDataUrlInput.Trim()))
        {
            _locationUrlStatus = "Enter a valid http:// or https:// URL.";
            return false;
        }
        LocationDataUrlInput = _settings.ExternalDataUrl;
        _locationUrlStatus = "Location data URL saved.";
        return true;
    }

    private bool SaveDungeonMapUrl()
    {
        if (!_plugin.SetDungeonMapUrl(DungeonMapUrlInput.Trim()))
        {
            _dungeonUrlStatus = "Enter a valid http:// or https:// URL.";
            return false;
        }
        DungeonMapUrlInput = _settings.DungeonMapUrl;
        _dungeonUrlStatus = "Dungeon map URL saved.";
        return true;
    }

    public IReadOnlyList<string> SearchResults
    {
        get
        {
            if (!_showSearchResults)
                return Array.Empty<string>();
            string query = (_searchingFrom ? FromInput : DestinationInput).Trim();
            if (query.Length == 0)
                return [CurrentLocation];
            var matches = _plugin.SearchLocations(query)
                .Where(location => location.HasCoordinates)
                .OrderBy(location => location.Name.StartsWith(query, StringComparison.OrdinalIgnoreCase) ? 0 : 1)
                .ThenBy(location => location.Name, StringComparer.OrdinalIgnoreCase)
                .Take(6).Select(location => location.Name).ToList();
            if (CurrentLocation.Contains(query, StringComparison.OrdinalIgnoreCase))
                matches.Insert(0, CurrentLocation);
            return matches;
        }
    }

    public bool SearchResultsVisible => _showSearchResults && SearchResults.Count > 0;

    public int SelectedSearchResult => -1;

    public string SearchResultsLabel => _searchingFrom ? "From matches" : "Destination matches";

    public IReadOnlyList<string> RouteSteps => _destination.CurrentRoute?.Steps
        .Select(step => string.IsNullOrWhiteSpace(StepDetailsLocation(step).Notes)
            ? step.ToString()
            : $"{step} [notes]")
        .ToArray() ?? [];

    private RouteStep? SelectedStep
    {
        get
        {
            Route? route = _destination.CurrentRoute;
            if (route is not { StepCount: > 0 })
            {
                _selectedRoute = null;
                _selectedRouteStep = 0;
                return null;
            }
            if (!ReferenceEquals(route, _selectedRoute))
            {
                _selectedRoute = route;
                _selectedRouteStep = 0;
            }
            _selectedRouteStep = Math.Clamp(_selectedRouteStep, 0, route.StepCount - 1);
            return route.Steps[_selectedRouteStep];
        }
    }

    public int SelectedRouteStep => SelectedStep is null ? -1 : _selectedRouteStep;

    public Action<int> SelectRouteStepAction => index =>
    {
        Route? route = _destination.CurrentRoute;
        if (route is null || index < 0 || index >= route.StepCount)
            return;
        _selectedRoute = route;
        _selectedRouteStep = index;
        ShowDetailsTab();
    };

    private static Location StepDetailsLocation(RouteStep step) =>
        step.Kind == RouteStepKind.Portal ? step.From : step.To;

    private Location? DetailsLocation => SelectedStep is { } step
        ? StepDetailsLocation(step) : null;

    public string DetailsStepNumberText => SelectedStep is null
        ? "Select a route step to see details."
        : $"Step {_selectedRouteStep + 1} of {_selectedRoute!.StepCount}";

    public string DetailsInstructionText => SelectedStep?.ToString() ?? string.Empty;
    public string DetailsLocationName => DetailsLocation?.Name ?? string.Empty;
    public string DetailsLocationType => DetailsLocation is { Type: not LocationType.Unknown } location
        ? location.Type.ToString() : "Unknown";
    public string DetailsCoordinates => DetailsLocation is { HasCoordinates: true } location
        ? location.Coords.ToString() : "Unknown";
    public bool DetailsArrivalVisible => SelectedStep is { Kind: RouteStepKind.Portal }
        || DetailsLocation?.HasExitCoords == true;
    public string DetailsArrivalCoordinates => SelectedStep is { Kind: RouteStepKind.Portal } step
        ? (step.From.HasExitCoords ? step.From.ExitCoords : step.To.Coords).ToString()
        : DetailsLocation?.ExitCoords.ToString() ?? string.Empty;
    public bool DetailsDistanceVisible => SelectedStep is { Kind: RouteStepKind.Travel };
    public string DetailsDistance => SelectedStep is { Kind: RouteStepKind.Travel } step
        ? TravelDistance.Format(step.Distance) : string.Empty;
    public int SelectedNotesLine => -1;
    public IReadOnlyList<string> DetailsNotesLines
    {
        get
        {
            Location? location = DetailsLocation;
            if (!ReferenceEquals(location, _notesLocation))
            {
                _notesLocation = location;
                _notesLines = WrapNotes(location?.Notes);
            }
            return _notesLines;
        }
    }

    private static IReadOnlyList<string> WrapNotes(string? notes)
    {
        if (string.IsNullOrWhiteSpace(notes))
            return ["No additional notes for this location."];
        var lines = new List<string>();
        foreach (string paragraph in notes.Replace("\r", string.Empty).Split('\n'))
        {
            var words = paragraph.Split(' ', StringSplitOptions.RemoveEmptyEntries);
            if (words.Length == 0)
            {
                lines.Add(string.Empty);
                continue;
            }
            string line = string.Empty;
            foreach (string word in words)
            {
                if (line.Length > 0 && line.Length + word.Length + 1 > 64)
                {
                    lines.Add(line);
                    line = string.Empty;
                }
                line = line.Length == 0 ? word : $"{line} {word}";
            }
            lines.Add(line);
        }
        return lines;
    }

    public string NextTargetText => _destination.CurrentRoute is { StepCount: > 0 }
        ? $"Next: {_destination.GetImmediateTarget()?.Name}"
        : "No route calculated";

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
        string value = DestinationInput.Trim();
        if (value.Equals(CurrentLocation, StringComparison.OrdinalIgnoreCase))
        {
            if (!_plugin.SetCurrentLocationDestination())
            {
                _host.Automation.Chat.PostSystemMessage("GoArrow: Current location is unavailable.");
                return;
            }
            DestinationInput = CurrentLocation;
        }
        else if (_plugin.TrySetCoordinateDestination(value) || _plugin.SetDestination(value))
        {
            DestinationInput = value;
        }
        else
        {
            _host.Automation.Chat.PostSystemMessage("GoArrow: Select a destination from the search results.");
            return;
        }
        _showSearchResults = false;
        _editingDestination = false;
    }

    public void SubmitFrom()
    {
        string value = FromInput.Trim();
        if (value.Length == 0 || value.Equals(CurrentLocation, StringComparison.OrdinalIgnoreCase))
        {
            _plugin.SetRouteFrom(null);
            FromInput = FromText = CurrentLocation;
        }
        else if (_plugin.SetRouteFrom(value))
            FromInput = FromText = value;
        else
        {
            _host.Automation.Chat.PostSystemMessage("GoArrow: Select a From location from the search results.");
            return;
        }
        _showSearchResults = false;
        _editingFrom = false;
    }

    public Action SubmitFromAction => SubmitFrom;

    public Action EditFromAction => () =>
    {
        FromEditorInput = string.Empty;
        _editingFrom = true;
    };

    public Action EditDestinationAction => () => _editingDestination = true;

    public Action<string> UpdateFromInputAction => text =>
    {
        FromInput = text;
        FromEditorInput = text;
        _editingFrom = true;
        _searchingFrom = true;
        _showSearchResults = true;
    };

    public Action<string> SubmitFromTextAction => text =>
    {
        FromInput = text;
        FromEditorInput = text;
        SubmitFrom();
    };

    /// <summary>Submit the destination field as a markup-compatible action.</summary>
    public Action SubmitDestinationAction => SubmitDestination;

    /// <summary>Keep the input value current when the field changes.</summary>
    public Action<string> UpdateDestinationInputAction => text =>
    {
        DestinationInput = text;
        _editingDestination = true;
        _searchingFrom = false;
        _showSearchResults = true;
    };

    /// <summary>Submit the text supplied by OpenAC's field callback.</summary>
    public Action<string> SubmitDestinationTextAction => text =>
    {
        DestinationInput = text;
        SubmitDestination();
    };

    public Action<int> SelectSearchResultAction => index =>
    {
        var results = SearchResults;
        if (index < 0 || index >= results.Count)
            return;
        if (_searchingFrom)
        {
            FromInput = results[index];
            SubmitFrom();
        }
        else
        {
            DestinationInput = results[index];
            SubmitDestination();
        }
    };

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

    public bool DungeonMapVisible => _plugin.DungeonMapVisible;
    public bool ArrowVisible => _settings.HudVisible;
    public bool ToolbarVisible => _settings.ToolbarVisible;

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

    /// <summary>Toggle route recalculation on/off.</summary>
    public Action ToggleRecalculateRoute => () => RecalculateRoute = !RecalculateRoute;

    public Action ToggleDungeonMap => () => _plugin.SetDungeonMapVisible(!DungeonMapVisible);
    public Action ToggleArrowVisible => () => _plugin.SetArrowVisible(!ArrowVisible);
    public Action ToggleToolbarVisible => () => _plugin.SetToolbarVisible(!ToolbarVisible);

    /// <summary>Compute and display the route; optionally start navigation.</summary>
    public Action StartNavigation => () =>
    {
        if (_destination.TargetName == CurrentLocation && !_plugin.SetCurrentLocationDestination())
        {
            _host.Automation.Chat.PostSystemMessage("GoArrow: Current location is unavailable.");
            return;
        }
        _plugin.Go();
    };

    /// <summary>Stop navigation.</summary>
    public Action StopNavigation => () => _plugin.StopNavigation();

    /// <summary>Resume after manually completing a portal or recall action.</summary>
    public Action ResumeNavigation => () => _navigator.ResumeAfterInteraction();

    /// <summary>Clear destination.</summary>
    public Action ClearDestination => () =>
    {
        _plugin.ClearDestination();
        DestinationInput = string.Empty;
        _editingDestination = true;
        _showSearchResults = false;
    };

    public Action<int> RemoveRouteStep => index => _plugin.RemoveRouteStep(index);
    public Action<int, int> MoveRouteStep => (from, to) => _plugin.MoveRouteStep(from, to);

    /// <summary>Show destination input hint.</summary>
    public Action ShowDestinationInput => () =>
        _host.Automation.Chat.PostSystemMessage("GoArrow: Use /go <destination> in chat to set a destination.");

    // ── Tick update ─────────────────────────────────────────────────

    public void OnTick(double elapsed)
    {
        if (!_showSearchResults && DestinationInput != _destination.TargetName
            && (_destination.HasDestination || _settings.DestinationName != CurrentLocation))
            DestinationInput = _destination.TargetName;
    }
}
