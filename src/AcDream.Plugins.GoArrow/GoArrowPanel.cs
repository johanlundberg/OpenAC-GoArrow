using AcDream.Plugin.Abstractions;

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

    /// <summary>Bearing to the next route waypoint (formatted).</summary>
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
            if (string.IsNullOrEmpty(_destination.TargetName))
                return "Idle";
            return _destination.CurrentRoute is { StepCount: > 0 } ? "Route ready" : "Ready";
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

    public string FromInput { get; set; } = CurrentLocation;

    public string FromEditorInput { get; private set; } = string.Empty;

    public string FromText { get; private set; } = CurrentLocation;

    public bool FromEditorVisible => _editingFrom;

    public bool FromSelectionVisible => !_editingFrom;

    public bool DestinationEditorVisible => _editingDestination;

    public bool DestinationSelectionVisible => !_editingDestination;

    public bool RouteTabSelected => !_showConfig;
    public bool ConfigTabSelected => _showConfig;
    public bool RouteTabVisible => !_showConfig;
    public bool ConfigTabVisible => _showConfig;
    public Action ShowRouteTab => () => _showConfig = false;
    public Action ShowConfigTab => () =>
    {
        _showSearchResults = false;
        _showConfig = true;
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

    public IReadOnlyList<string> RouteSteps => _plugin.GetCurrentRouteSteps();

    public int SelectedRouteStep => _destination.CurrentRoute is { StepCount: > 0 } ? 0 : -1;

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
