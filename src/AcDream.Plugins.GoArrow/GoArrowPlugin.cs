using System.Reflection;
using AcDream.Plugin.Abstractions;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow;

/// <summary>
/// GoArrow Plugin for OpenAC.
/// Provides destination-based navigation assistance with route finding,
/// chat commands, and a declarative UI panel.
/// </summary>
public sealed class GoArrowPlugin : IAcDreamPlugin
{
    private IPluginHost? _host;
    private GoArrowSettings? _settings;
    private LocationDatabase? _database;
    private RouteFinder? _routeFinder;
    private GoArrowDestination? _destination;
    private GoArrowNavigator? _navigator;
    private GoArrowCommands? _commands;
    private GoArrowPanel? _panel;
    private GoArrowHud? _hud;
    private GoArrowMap? _map;
    private WarcryAtlasDataProvider? _atlasProvider;
    private int _atlasUpdateInProgress;
    private IDisposable? _commandRegistration;
    private Action<double>? _tickHandler;
    private PluginChatCoordinateLinkRouter? _coordinateLinkRouter;
    private Action<SelectionChangedEvent>? _selectionChangedHandler;
    private Action<PluginPortalTransition>? _portalTransitionHandler;
    private long _recallRequestRevision;

    /// <summary>
    /// The name of the current destination, or empty.
    /// </summary>
    internal string CurrentDestinationName => _destination?.TargetName ?? string.Empty;

    public void Initialize(IPluginHost host)
    {
        _host = host;

        // ── Load settings ───────────────────────────────────────────
        _settings = new GoArrowSettings();
        _settings.Load(host.Storage);
        LoadScopedRecallState(host);

        // ── Initialize database and load embedded data ──────────────
        _database = new LocationDatabase();
        LoadEmbeddedData();
        LoadLayeredData();
        _atlasProvider = new WarcryAtlasDataProvider(host.Storage, url: _settings.ExternalDataUrl);

        // ── Initialize route finding ───────────────────────────────
        _routeFinder = new RouteFinder(_database);

        // ── Restore previous destination if set ─────────────────────
        _destination = new GoArrowDestination(_settings, _database, _routeFinder);
        if (!string.IsNullOrEmpty(_settings.DestinationName))
        {
            _destination.SetDestination(_settings.DestinationName);
        }

        // ── Initialize navigator ───────────────────────────────────
        _navigator = new GoArrowNavigator(host, _destination, _settings);

        // ── Initialize commands ────────────────────────────────────
        _commands = new GoArrowCommands(host, this);

        // ── Initialize panel ───────────────────────────────────────
        _panel = new GoArrowPanel(host, this, _settings, _destination, _navigator);

        host.Log.Info("GoArrow initialized");
    }

    public void Enable()
    {
        if (_host is null || _panel is null)
            return;

        // Register the declarative panel
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location) ?? ".";

        _host.Ui.AddPanel(
            new PluginPanelDescriptor("main", "GoArrow")
            {
                IconText = "GA",
                StartVisible = _settings?.PanelVisible ?? true,
                ShowInSidePanel = true,
            },
            Path.Combine(directory, "goarrow-panel.xml"),
            _panel);

        // Register the "/go" chat command
        if (_commands is not null)
            _commandRegistration = _host.Commands.Register(new GoArrowCommandDefinition(_commands, this));

        // Coordinate links are host-owned events; the router is disposed with the plugin.
        _coordinateLinkRouter = new PluginChatCoordinateLinkRouter(
            _host.Automation.Chat,
            coordinate => SetCoordinateDestination(coordinate.NorthSouth, coordinate.EastWest,
                $"{coordinate.NorthSouth:0.###}N {coordinate.EastWest:0.###}E"));
        _selectionChangedHandler = OnSelectionChanged;
        _host.Selection.Changed += _selectionChangedHandler;
        _portalTransitionHandler = OnPortalTransition;
        _host.Events.PortalTransition += _portalTransitionHandler;

        // Subscribe to navigation reports and tick events
        _navigator?.Enable();
        if (_destination is not null && _navigator is not null)
        {
            _hud = new GoArrowHud(_host, _destination, _navigator, _settings!);
            _hud.Enable();
            _map = new GoArrowMap(_host, _destination, _settings!);
            _map.Enable();
        }
        _tickHandler = OnTick;
        _host.Events.Tick += _tickHandler;

        _host.Log.Info(
            _host.Automation.IsAvailable
                ? "GoArrow enabled"
                : "GoArrow enabled (no live session yet; navigation unavailable)");
    }

    public void Disable()
    {
        if (_host is not null && _tickHandler is not null)
            _host.Events.Tick -= _tickHandler;

        if (_host is not null && _selectionChangedHandler is not null)
            _host.Selection.Changed -= _selectionChangedHandler;
        _coordinateLinkRouter?.Dispose();
        _coordinateLinkRouter = null;
        _selectionChangedHandler = null;
        if (_host is not null && _portalTransitionHandler is not null)
            _host.Events.PortalTransition -= _portalTransitionHandler;
        _portalTransitionHandler = null;

        _map?.Dispose();
        _map = null;
        _hud?.Dispose();
        _hud = null;
        _navigator?.Disable();
        _navigator?.StopNavigation();
        _commandRegistration?.Dispose();
        _commandRegistration = null;
        _tickHandler = null;

        // Save settings on disable
        _settings?.Save(_host?.Storage!);

        _host?.Log.Info("GoArrow disabled");
    }

    /// <summary>
    /// Set a destination by name. Returns true if found.
    /// </summary>
    internal bool SetDestination(string name)
    {
        if (_destination == null || _host == null)
            return false;

        bool found = _destination.SetDestination(name);
        if (found)
        {
            _settings?.Save(_host.Storage);

            // Auto-start navigation if enabled
            if (_settings?.AutoNavigate == true)
            {
                _navigator?.StartNavigation();
            }

            _host.Automation.Chat.PostSystemMessage($"GoArrow: Destination '{name}' set.");
        }
        return found;
    }

    /// <summary>
    /// Clear the current destination.
    /// </summary>
    internal void ClearDestination()
    {
        _navigator?.StopNavigation();
        _destination?.ClearDestination();
        _settings?.Save(_host?.Storage!);
    }

    /// <summary>
    /// Stop active navigation.
    /// </summary>
    internal void StopNavigation()
    {
        _navigator?.StopNavigation();
    }

    /// <summary>Show a route and start walking only when auto-navigation is enabled.</summary>
    internal void Go()
    {
        if (_navigator is null)
            return;
        if (_settings?.AutoNavigate == true)
        {
            _navigator.StartNavigation();
            return;
        }

        if (_navigator.IsNavigating)
            _navigator.StopNavigation();
        _navigator.PlanRoute();
    }

    /// <summary>
    /// Resume after manually completing a portal or recall interaction.
    /// </summary>
    internal void ResumeNavigation()
    {
        _navigator?.ResumeAfterInteraction();
    }

    /// <summary>
    /// Loads a validated XML location database from a relative plugin-storage
    /// file name such as <c>filename.xml</c>; the plugin prepends its
    /// hardcoded <c>GoArrow/</c> storage directory.
    /// </summary>
    internal bool LoadDataFile(string storageKey)
    {
        if (_host is null || _database is null || !_host.Storage.IsAvailable)
            return false;

        string relativePath = storageKey.Replace('\\', '/');
        if (relativePath.StartsWith("/", StringComparison.Ordinal)
            || relativePath.Contains("..", StringComparison.Ordinal)
            || relativePath.StartsWith("GoArrow/", StringComparison.OrdinalIgnoreCase)
            || !relativePath.EndsWith(".xml", StringComparison.OrdinalIgnoreCase))
            return false;

        // IPluginStorage is already scoped to this plugin, but GoArrow keeps
        // imported data beneath its own stable subdirectory.
        string normalized = $"GoArrow/{relativePath}";
        string? xml = _host.Storage.ReadText(normalized);
        if (string.IsNullOrWhiteSpace(xml))
            return false;

        try
        {
            var candidate = new LocationDatabase();
            candidate.LoadLocationsXml(xml);
            if (candidate.LocationCount == 0)
                return false;

            _database.LoadLocationsXml(xml);
            _routeFinder?.InvalidateGraph();
            _host.Log.Info($"GoArrow: Loaded {_database.LocationCount} locations from storage key '{normalized}'.");
            return true;
        }
        catch (Exception exception)
        {
            _host.Log.Error($"GoArrow: Failed to load location data from '{normalized}'.", exception);
            return false;
        }
    }

    /// <summary>
    /// Changes and persists the URL used by the explicit location-data update.
    /// </summary>
    internal bool SetExternalDataUrl(string url)
    {
        if (_settings is null || _host is null || !Uri.TryCreate(url, UriKind.Absolute, out Uri? parsed)
            || (parsed.Scheme != Uri.UriSchemeHttp && parsed.Scheme != Uri.UriSchemeHttps))
            return false;

        _settings.ExternalDataUrl = parsed.ToString();
        _atlasProvider = new WarcryAtlasDataProvider(_host.Storage, url: _settings.ExternalDataUrl);
        _settings.Save(_host.Storage);
        return true;
    }

    /// <summary>
    /// Explicitly downloads and installs the latest Warcry Atlas location data.
    /// The update is intentionally never started automatically during startup.
    /// </summary>
    internal async Task UpdateDataAsync()
    {
        if (_host is null || _database is null || _atlasProvider is null)
            return;
        if (Interlocked.Exchange(ref _atlasUpdateInProgress, 1) != 0)
        {
            _host.Automation.Chat.PostSystemMessage("GoArrow: A location-data update is already running.");
            return;
        }

        try
        {
            _host.Automation.Chat.PostSystemMessage("GoArrow: Downloading location data...");
            string xml = await _atlasProvider.DownloadAsync().ConfigureAwait(false);
            _database.LoadLocationsXml(xml);
            _routeFinder?.InvalidateGraph();
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Loaded {_database.LocationCount} locations from the Atlas data source.");
        }
        catch (Exception exception)
        {
            _host.Log.Error("GoArrow: Location-data update failed.", exception);
            string? cached = null;
            try
            {
                TimeSpan? maxAge = _settings?.AtlasCacheMaxAgeDays > 0
                    ? TimeSpan.FromDays(_settings.AtlasCacheMaxAgeDays)
                    : null;
                cached = _atlasProvider.ReadCached(maxAge);
            }
            catch (Exception cacheException)
            {
                _host.Log.Warn($"GoArrow: Cached location data is invalid: {cacheException.Message}");
            }

            if (cached is not null)
            {
                _database.LoadLocationsXml(cached);
                _routeFinder?.InvalidateGraph();
                _host.Automation.Chat.PostSystemMessage(
                    $"GoArrow: Download failed; loaded {_database.LocationCount} cached locations.");
            }
            else
            {
                _host.Automation.Chat.PostSystemMessage(
                    "GoArrow: Location-data update failed; keeping the existing database.");
            }
        }
        finally
        {
            Volatile.Write(ref _atlasUpdateInProgress, 0);
        }
    }

    /// <summary>
    /// Get all destination names from the database.
    /// </summary>
    internal List<string> GetAllDestinationNames()
    {
        return _destination?.GetAllDestinationNames() ?? new List<string>();
    }

    /// <summary>
    /// Get favorite destinations.
    /// </summary>
    internal List<string> GetFavorites()
    {
        return _settings?.FavoriteDestinations ?? new List<string>();
    }

    internal bool RemoveRouteStep(int index) => _destination?.RemoveRouteStep(index) == true;

    internal bool MoveRouteStep(int fromIndex, int toIndex) => _destination?.MoveRouteStep(fromIndex, toIndex) == true;

    internal IReadOnlyList<string> GetCurrentRouteSteps()
    {
        return _destination?.CurrentRoute?.Steps
            .Select(step => step.ToString())
            .ToArray()
            ?? Array.Empty<string>();
    }

    internal IReadOnlyList<Location> SearchLocations(string query)
    {
        if (_routeFinder is null)
            return Array.Empty<Location>();
        return _routeFinder.SearchLocations(query);
    }

    internal string CurrentPositionText(string prefix = "GoArrow: Current position")
    {
        if (_host is null || !_host.Automation.IsAvailable)
            return $"{prefix} is unavailable (not in world).";

        var position = _host.Automation.Navigation.Snapshot.Position;
        var coordinates = new Coordinates(position.NorthSouth, position.EastWest);
        return $"{prefix}: {coordinates}";
    }

    internal bool TrySetCoordinateDestination(string text)
    {
        if (!PluginChatCoordinateParser.TryParse(text, out PluginChatCoordinate coordinate))
            return false;
        SetCoordinateDestination(coordinate.NorthSouth, coordinate.EastWest, text);
        return true;
    }

    private void SetCoordinateDestination(double northSouth, double eastWest, string displayText)
    {
        if (_destination is null || _host is null)
            return;
        _destination.SetCoordinate(northSouth, eastWest, displayText);
        _settings?.Save(_host.Storage);
        if (_settings?.AutoNavigate == true)
            _navigator?.StartNavigation();
    }

    internal void Recall(string value)
    {
        if (_host is null)
            return;
        if (!Enum.TryParse(value, true, out PluginRecallKind kind))
        {
            _host.Automation.Chat.PostSystemMessage("GoArrow: Recall must be lifestone, marketplace, house, mansion, or allegiance.");
            return;
        }
        if (!_host.Automation.Recalls.IsAvailable)
        {
            _host.Automation.Chat.PostSystemMessage("GoArrow: Recall is unavailable outside a live session.");
            return;
        }
        PluginRecallResult result = _host.Automation.Recalls.Recall(kind);
        if (!result.Accepted)
        {
            _host.Automation.Chat.PostSystemMessage($"GoArrow: Recall was not accepted ({result.Status}).");
            return;
        }
        _recallRequestRevision = _host.Automation.Recalls.LastRequest.Revision;
        _host.Automation.Chat.PostSystemMessage($"GoArrow: {kind} recall started.");
    }

    private void OnPortalTransition(PluginPortalTransition transition)
    {
        if (_host is null || !transition.IsCompleted || transition.RecallRequestRevision == 0
            || transition.RecallRequestRevision != _recallRequestRevision)
            return;
        PluginRecallLocation known = _host.Automation.Recalls.CaptureLocations()
            .FirstOrDefault(item => item.Kind == _host.Automation.Recalls.LastRequest.Kind && item.IsKnown);
        if (!known.IsKnown)
            return;
        string value = $"{known.Position.NorthSouth.ToString(System.Globalization.CultureInfo.InvariantCulture)},{known.Position.EastWest.ToString(System.Globalization.CultureInfo.InvariantCulture)}";
        switch (known.Kind)
        {
            case PluginRecallKind.Lifestone: _settings!.LastPortalRecall = value; break;
            case PluginRecallKind.Marketplace: _settings!.LastSecondaryRecall = value; break;
            case PluginRecallKind.Allegiance: _settings!.LastAllegianceRecall = value; break;
            case PluginRecallKind.House: _settings!.LastHouseRecall = value; break;
            case PluginRecallKind.Mansion: _settings!.LastMansionRecall = value; break;
        }
        _settings?.Save(_host.Storage);
        if (_host.SessionSettings.TryGetValue("characterId", out string? characterId)
            && _host.SessionSettings.TryGetValue("worldId", out string? worldId)
            && !string.IsNullOrWhiteSpace(characterId) && !string.IsNullOrWhiteSpace(worldId))
        {
            IPluginStorage scoped = _host.Storage
                .OpenScope(PluginStorageScope.Character(characterId))
                .OpenScope(PluginStorageScope.World(worldId));
            scoped.WriteText($"recall/{known.Kind.ToString().ToLowerInvariant()}", value);
        }
    }

    private void LoadScopedRecallState(IPluginHost host)
    {
        if (_settings is null || !host.SessionSettings.TryGetValue("characterId", out string? characterId)
            || !host.SessionSettings.TryGetValue("worldId", out string? worldId)
            || string.IsNullOrWhiteSpace(characterId) || string.IsNullOrWhiteSpace(worldId))
            return;
        IPluginStorage scoped = host.Storage
            .OpenScope(PluginStorageScope.Character(characterId))
            .OpenScope(PluginStorageScope.World(worldId));
        _settings.LastPortalRecall = scoped.ReadText("recall/lifestone") ?? _settings.LastPortalRecall;
        _settings.LastSecondaryRecall = scoped.ReadText("recall/marketplace") ?? _settings.LastSecondaryRecall;
        _settings.LastHouseRecall = scoped.ReadText("recall/house") ?? _settings.LastHouseRecall;
        _settings.LastMansionRecall = scoped.ReadText("recall/mansion") ?? _settings.LastMansionRecall;
        _settings.LastAllegianceRecall = scoped.ReadText("recall/allegiance") ?? _settings.LastAllegianceRecall;
    }

    internal bool SetSelectedObjectDestination()
    {
        if (_host is null || !_host.Automation.IsAvailable || _destination is null)
            return false;
        uint? selected = _host.Selection.SelectedObjectId;
        if (selected is not { } objectId
            || !_host.Automation.Objects.TryGet(objectId, out PluginWorldObject obj)
            || !_destination.SetObject(obj))
            return false;
        _settings?.Save(_host.Storage);
        if (_settings?.AutoNavigate == true)
            _navigator?.StartNavigation();
        return true;
    }

    internal void SetNavigationLock(bool locked)
    {
        if (_settings is null || _host is null)
            return;
        _settings.NavigationLocked = locked;
        _settings.Save(_host.Storage);
    }

    private void OnSelectionChanged(SelectionChangedEvent change)
    {
        if (_destination?.Kind == GoArrowDestinationKind.Object
            && _destination.TargetObjectId is { } target
            && change.SelectedObjectId != target)
            _destination.MarkObjectUnavailable();
    }

    internal string DestinationPositionText()
    {
        if (_destination?.TargetLocation is not Location location)
            return "GoArrow: No destination set.";
        return $"GoArrow: Destination '{location.Name}': {location.Coords}";
    }

    /// <summary>
    /// Add a destination to favorites.
    /// </summary>
    internal void AddFavorite(string name)
    {
        if (_settings == null || _host == null)
            return;

        if (!_settings.FavoriteDestinations.Contains(name, StringComparer.OrdinalIgnoreCase))
        {
            _settings.FavoriteDestinations.Add(name);
            _settings.Save(_host.Storage);
        }
    }

    private void OnTick(double elapsed)
    {
        if (_host == null || _destination == null || _navigator == null)
            return;

        // Update current position from navigation snapshot
        if (_host.Automation.IsAvailable)
        {
            var position = _host.Automation.Navigation.Snapshot.Position;
            _navigator.UpdatePosition(position);

            // Recalculate while idle. During navigation the navigator owns
            // the current route and advances it from navigation reports.
            if (_destination.HasDestination && !_navigator.IsNavigating && !_navigator.WaitingForInteraction && !_navigator.HasArrived
                && (_destination.CurrentRoute is null || _settings?.RecalculateRoute == true))
            {
                var currentLoc = new RouteFinding.Location(
                    "Current Position",
                    position.NorthSouth,
                    position.EastWest);
                _destination.CalculateRoute(currentLoc);
            }
            else if (_destination.HasDestination)
            {
                var currentLoc = new RouteFinding.Location(
                    "Current Position",
                    position.NorthSouth,
                    position.EastWest);
                _destination.UpdateGuidance(currentLoc);
            }
        }

        // Tick navigator
        _navigator.OnTick(elapsed);
    }

    private void LoadLayeredData()
    {
        if (_host is null || _database is null)
            return;
        // The catalog returns the host's deterministic embedded/installed/user
        // precedence order; later layers override earlier records.
        foreach (string resourceId in _host.Resources.ListDataFiles("data")
            .Where(id => id.EndsWith(".xml", StringComparison.OrdinalIgnoreCase)))
        {
            try
            {
                using Stream? stream = _host.Resources.OpenRead(resourceId);
                if (stream is null) continue;
                using var reader = new StreamReader(stream);
                string xml = reader.ReadToEnd();
                var candidate = new LocationDatabase();
                candidate.LoadLocationsXml(xml);
                if (candidate.LocationCount == 0) continue;
                _database.LoadLocationsXml(xml);
                _host.Log.Info($"GoArrow: Loaded layered data '{resourceId}' ({candidate.LocationCount} locations).");
            }
            catch (Exception exception)
            {
                _host.Log.Warn($"GoArrow: Ignoring invalid layered data '{resourceId}': {exception.Message}");
            }
        }
    }

    private void LoadEmbeddedData()
    {
        if (_host == null || _database == null)
            return;

        var assembly = typeof(GoArrowPlugin).Assembly;
        var resources = assembly.GetManifestResourceNames();

        foreach (var res in resources)
        {
            if (res.EndsWith("DefaultLocations.xml", StringComparison.OrdinalIgnoreCase))
            {
                using var stream = assembly.GetManifestResourceStream(res);
                if (stream != null)
                {
                    using var reader = new StreamReader(stream);
                    var xml = reader.ReadToEnd();
                    _database.LoadLocationsXml(xml);
                    _host.Log.Info($"GoArrow: Loaded {_database.LocationCount} locations.");
                }
            }

            if (res.EndsWith("DefaultPortalDevices.xml", StringComparison.OrdinalIgnoreCase))
            {
                using var stream = assembly.GetManifestResourceStream(res);
                if (stream != null)
                {
                    using var reader = new StreamReader(stream);
                    var xml = reader.ReadToEnd();
                    _database.LoadPortalDevicesXml(xml);
                }
            }

            if (res.EndsWith("DefaultRouteStarts.xml", StringComparison.OrdinalIgnoreCase))
            {
                using var stream = assembly.GetManifestResourceStream(res);
                if (stream != null)
                {
                    using var reader = new StreamReader(stream);
                    var xml = reader.ReadToEnd();
                    _database.LoadRouteStartsXml(xml);
                }
            }
        }
    }
}
