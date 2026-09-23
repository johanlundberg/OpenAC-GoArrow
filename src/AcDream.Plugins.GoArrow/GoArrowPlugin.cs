using System.Diagnostics;
using System.Reflection;
using System.Xml;
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
    internal static string DisplayTitle
    {
        get
        {
            string version =
                typeof(GoArrowPlugin)
                    .Assembly.GetCustomAttribute<AssemblyInformationalVersionAttribute>()
                    ?.InformationalVersion.Split('+')[0]
                ?? typeof(GoArrowPlugin).Assembly.GetName().Version?.ToString(3)
                ?? "unknown";
            return $"GoArrow v{version}";
        }
    }

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
    private GoArrowDungeonMap? _dungeonMap;
    private WarcryAtlasDataProvider? _atlasProvider;
    private int _atlasUpdateInProgress;
    private int _dungeonUpdateInProgress;
    private int _pendingDungeonMapReload;
    private int _downloadedDungeonMapCount;
    private IDisposable? _commandRegistration;
    private Action<double>? _tickHandler;
    private PluginChatCoordinateLinkRouter? _coordinateLinkRouter;
    private Action<SelectionChangedEvent>? _selectionChangedHandler;
    private Action<PluginPortalTransition>? _portalTransitionHandler;
    private long _recallRequestRevision;
    private Location? _routeFromOverride;
    private PendingRouteWork _pendingRouteWork;
    private long _routeWorkReadyAt;
    private bool _routeWorkInProgress;
    private PluginNavigationPosition? _lastPreviewPosition;
    private long _lastPreviewAt;
    private const string IndoorLocationsStorageKey = "GoArrow/indoor-locations.xml";

    private enum PendingRouteWork
    {
        None,
        Go,
        Preview,
        Resume,
    }

    internal bool IsComputingRoute =>
        _pendingRouteWork != PendingRouteWork.None || _routeWorkInProgress;

    /// <summary>
    /// The name of the current destination, or empty.
    /// </summary>
    internal string CurrentDestinationName => _destination?.TargetName ?? string.Empty;
    internal string NavigationFailureReason => _navigator?.FailureReason ?? string.Empty;
    internal GoArrowPanel? Panel => _panel;
    internal string LocationDownloadStatus { get; private set; } = string.Empty;
    internal string DungeonDownloadStatus { get; private set; } = string.Empty;

    internal void ResetOverlayPositions()
    {
        if (_settings is null || _host is null)
            return;
        _settings.ResetOverlayPositions();
        _hud?.ResetPositions();
        _dungeonMap?.ResetPosition();
        _settings.Save(_host.Storage);
    }

    internal void SetArrowVisible(bool visible)
    {
        if (_settings is null || _host is null)
            return;
        _settings.HudVisible = visible;
        _hud?.SetArrowVisible(visible);
        _settings.Save(_host.Storage);
    }

    internal void SetToolbarVisible(bool visible)
    {
        if (_settings is null || _host is null)
            return;
        _settings.ToolbarVisible = visible;
        _hud?.SetToolbarVisible(visible);
        _settings.Save(_host.Storage);
    }

    public void Initialize(IPluginHost host)
    {
        _host = host;

        // ── Load settings ───────────────────────────────────────────
        _settings = new GoArrowSettings();
        _settings.Load(host.Storage);
        LoadScopedRecallState(host);

        // ── Initialize database and load embedded data ──────────────
        _database = new LocationDatabase();
        _atlasProvider = new WarcryAtlasDataProvider(host.Storage, url: _settings.ExternalDataUrl);
        LoadEmbeddedData();
        LoadCachedAtlasData();
        LoadLayeredData();
        LoadSavedIndoorLocations();

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
            new PluginPanelDescriptor("main", DisplayTitle)
            {
                IconText = "GA",
                StartVisible = _settings?.PanelVisible ?? true,
                ShowInSidePanel = true,
            },
            Path.Combine(directory, "goarrow-panel.xml"),
            _panel
        );

        // Register the "/go" chat command
        if (_commands is not null)
            _commandRegistration = _host.Commands.Register(
                new GoArrowCommandDefinition(_commands, this)
            );

        // Coordinate links are host-owned events; the router is disposed with the plugin.
        _coordinateLinkRouter = new PluginChatCoordinateLinkRouter(
            _host.Automation.Chat,
            coordinate =>
                SetCoordinateDestination(
                    coordinate.NorthSouth,
                    coordinate.EastWest,
                    $"{coordinate.NorthSouth:0.###}N {coordinate.EastWest:0.###}E"
                )
        );
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
            ReloadDungeonMaps();
        }
        _tickHandler = OnTick;
        _host.Events.Tick += _tickHandler;

        _host.Log.Info(
            _host.Automation.IsAvailable
                ? "GoArrow enabled"
                : "GoArrow enabled (no live session yet; navigation unavailable)"
        );
    }

    public void Disable()
    {
        _pendingRouteWork = PendingRouteWork.None;
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

        _dungeonMap?.Dispose();
        _dungeonMap = null;
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
            _pendingRouteWork = PendingRouteWork.None;
            _lastPreviewAt = 0;
            _navigator?.StopNavigation();
            _settings?.Save(_host.Storage);
            _host.Automation.Chat.PostSystemMessage($"GoArrow: Destination '{name}' set.");
        }
        return found;
    }

    /// <summary>
    /// Clear the current destination.
    /// </summary>
    internal void ClearDestination()
    {
        _pendingRouteWork = PendingRouteWork.None;
        _lastPreviewAt = 0;
        _navigator?.StopNavigation();
        _destination?.ClearDestination();
        _settings?.Save(_host?.Storage!);
    }

    /// <summary>
    /// Stop active navigation.
    /// </summary>
    internal void StopNavigation()
    {
        _pendingRouteWork = PendingRouteWork.None;
        _navigator?.StopNavigation();
    }

    /// <summary>Show a route and start walking only when auto-navigation is enabled.</summary>
    internal void Go()
    {
        if (_navigator is null)
            return;
        if (_host?.HasUi == true && _tickHandler is not null)
        {
            QueueRouteWork(PendingRouteWork.Go);
            return;
        }
        ExecuteGo();
    }

    private void ExecuteGo()
    {
        if (_navigator is null)
            return;
        try
        {
            if (_routeFromOverride is { } from)
            {
                _navigator.StopNavigation();
                _destination?.CalculateRoute(from);
                return;
            }
            if (_destination?.TargetName == "Current Location")
            {
                _navigator.PlanRoute();
                return;
            }
            if (_settings?.AutoNavigate == true)
            {
                _navigator.StartNavigation();
                return;
            }

            if (_navigator.IsNavigating)
                _navigator.StopNavigation();
            _navigator.PlanRoute();
        }
        finally
        {
            if (_host?.Automation.IsAvailable == true)
            {
                var snapshot = _host.Automation.Navigation.Snapshot;
                if (snapshot.IsAvailable)
                {
                    _lastPreviewPosition = snapshot.Position;
                    _lastPreviewAt = Stopwatch.GetTimestamp();
                }
            }
        }
    }

    private void QueueRouteWork(PendingRouteWork work)
    {
        _pendingRouteWork = work;
        // Let at least one drawn frame show the status before a large graph
        // build blocks the UI thread.
        _routeWorkReadyAt = Stopwatch.GetTimestamp() + (long)(Stopwatch.Frequency * 0.05);
    }

    internal bool SetRouteFrom(string? name)
    {
        _pendingRouteWork = PendingRouteWork.None;
        _lastPreviewAt = 0;
        if (name is null)
        {
            _routeFromOverride = null;
            _destination?.ClearRoute();
            return true;
        }
        Location? location = _database?.FindLocation(name);
        if (location is null || !location.HasCoordinates)
            return false;
        _navigator?.StopNavigation();
        _routeFromOverride = location;
        _destination?.ClearRoute();
        return true;
    }

    internal bool SetCurrentLocationDestination()
    {
        if (_host is null || _destination is null || !_host.Automation.IsAvailable)
            return false;
        var snapshot = _host.Automation.Navigation.Snapshot;
        if (!snapshot.IsAvailable)
            return false;
        _navigator?.StopNavigation();
        _destination.SetCoordinate(
            snapshot.Position.NorthSouth,
            snapshot.Position.EastWest,
            "Current Location"
        );
        _settings?.Save(_host.Storage);
        return true;
    }

    /// <summary>
    /// Resume a stopped route or a manually completed portal/recall interaction.
    /// </summary>
    internal void ResumeNavigation()
    {
        if (_navigator?.CanResumeNavigation != true)
            return;
        if (_navigator.WaitingForInteraction)
        {
            _navigator.ResumeNavigation();
            return;
        }
        if (_host?.HasUi == true && _tickHandler is not null)
        {
            QueueRouteWork(PendingRouteWork.Resume);
            return;
        }
        _navigator.ResumeNavigation();
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
        if (
            relativePath.StartsWith("/", StringComparison.Ordinal)
            || relativePath.Contains("..", StringComparison.Ordinal)
            || relativePath.StartsWith("GoArrow/", StringComparison.OrdinalIgnoreCase)
            || !relativePath.EndsWith(".xml", StringComparison.OrdinalIgnoreCase)
        )
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
            _host.Log.Info(
                $"GoArrow: Loaded {_database.LocationCount} locations from storage key '{normalized}'."
            );
            return true;
        }
        catch (Exception exception)
        {
            _host.Log.Error(
                $"GoArrow: Failed to load location data from '{normalized}'.",
                exception
            );
            return false;
        }
    }

    /// <summary>
    /// Changes and persists the URL used by the explicit location-data update.
    /// </summary>
    internal bool SetExternalDataUrl(string url)
    {
        if (_settings is null || _host is null || !TryDownloadUrl(url, out Uri? parsed))
            return false;

        _settings.ExternalDataUrl = parsed!.ToString();
        _atlasProvider = new WarcryAtlasDataProvider(_host.Storage, url: _settings.ExternalDataUrl);
        _settings.Save(_host.Storage);
        return true;
    }

    internal bool SetDungeonMapUrl(string url)
    {
        if (_settings is null || _host is null || !TryDownloadUrl(url, out Uri? parsed))
            return false;
        _settings.DungeonMapUrl = parsed!.ToString();
        _settings.Save(_host.Storage);
        return true;
    }

    private static bool TryDownloadUrl(string url, out Uri? parsed) =>
        Uri.TryCreate(url.Trim(), UriKind.Absolute, out parsed)
        && (parsed.Scheme == Uri.UriSchemeHttp || parsed.Scheme == Uri.UriSchemeHttps);

    /// <summary>
    /// Explicitly downloads and installs the latest Warcry Atlas location data.
    /// The update is intentionally never started automatically during startup.
    /// </summary>
    internal async Task UpdateDataAsync()
    {
        if (_host is null || _database is null || _atlasProvider is null)
            return;
        if (string.IsNullOrWhiteSpace(_settings?.ExternalDataUrl))
        {
            LocationDownloadStatus = "Set a location data URL first.";
            _host.Automation.Chat.PostSystemMessage(
                "GoArrow: Set a location data URL in Config before downloading."
            );
            return;
        }
        if (Interlocked.Exchange(ref _atlasUpdateInProgress, 1) != 0)
        {
            _host.Automation.Chat.PostSystemMessage(
                "GoArrow: A location-data update is already running."
            );
            return;
        }

        try
        {
            LocationDownloadStatus = "Downloading location data...";
            _host.Automation.Chat.PostSystemMessage("GoArrow: Downloading location data...");
            string xml = await _atlasProvider.DownloadAsync().ConfigureAwait(false);
            _database.LoadLocationsXml(xml);
            _routeFinder?.InvalidateGraph();
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Loaded {_database.LocationCount} locations from the Atlas data source."
            );
            LocationDownloadStatus = $"Loaded {_database.LocationCount} locations.";
        }
        catch (Exception exception)
        {
            _host.Log.Error("GoArrow: Location-data update failed.", exception);
            string? cached = null;
            try
            {
                TimeSpan? maxAge =
                    _settings?.AtlasCacheMaxAgeDays > 0
                        ? TimeSpan.FromDays(_settings.AtlasCacheMaxAgeDays)
                        : null;
                cached = _atlasProvider.ReadCached(maxAge);
            }
            catch (Exception cacheException)
            {
                _host.Log.Warn(
                    $"GoArrow: Cached location data is invalid: {cacheException.Message}"
                );
            }

            if (cached is not null)
            {
                _database.LoadLocationsXml(cached);
                _routeFinder?.InvalidateGraph();
                _host.Automation.Chat.PostSystemMessage(
                    $"GoArrow: Download failed; loaded {_database.LocationCount} cached locations."
                );
                LocationDownloadStatus = "Download failed; loaded cached data.";
            }
            else
            {
                _host.Automation.Chat.PostSystemMessage(
                    "GoArrow: Location-data update failed; keeping the existing database."
                );
                LocationDownloadStatus = "Download failed; existing data kept.";
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

    internal bool MoveRouteStep(int fromIndex, int toIndex) =>
        _destination?.MoveRouteStep(fromIndex, toIndex) == true;

    internal IReadOnlyList<string> GetCurrentRouteSteps()
    {
        return _destination?.CurrentRoute?.Steps.Select(step => step.ToString()).ToArray()
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
        _navigator?.StopNavigation();
        _destination.SetCoordinate(northSouth, eastWest, displayText);
        _settings?.Save(_host.Storage);
    }

    internal void Recall(string value)
    {
        if (_host is null)
            return;
        if (!Enum.TryParse(value, true, out PluginRecallKind kind))
        {
            _host.Automation.Chat.PostSystemMessage(
                "GoArrow: Recall must be lifestone, marketplace, house, mansion, or allegiance."
            );
            return;
        }
        if (!_host.Automation.Recalls.IsAvailable)
        {
            _host.Automation.Chat.PostSystemMessage(
                "GoArrow: Recall is unavailable outside a live session."
            );
            return;
        }
        PluginRecallResult result = _host.Automation.Recalls.Recall(kind);
        if (!result.Accepted)
        {
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Recall was not accepted ({result.Status})."
            );
            return;
        }
        _recallRequestRevision = _host.Automation.Recalls.LastRequest.Revision;
        _host.Automation.Chat.PostSystemMessage($"GoArrow: {kind} recall started.");
    }

    private void OnPortalTransition(PluginPortalTransition transition)
    {
        if (
            _host is null
            || !transition.IsCompleted
            || transition.RecallRequestRevision == 0
            || transition.RecallRequestRevision != _recallRequestRevision
        )
            return;
        PluginRecallLocation known = _host
            .Automation.Recalls.CaptureLocations()
            .FirstOrDefault(item =>
                item.Kind == _host.Automation.Recalls.LastRequest.Kind && item.IsKnown
            );
        if (!known.IsKnown)
            return;
        string value =
            $"{known.Position.NorthSouth.ToString(System.Globalization.CultureInfo.InvariantCulture)},{known.Position.EastWest.ToString(System.Globalization.CultureInfo.InvariantCulture)}";
        switch (known.Kind)
        {
            case PluginRecallKind.Lifestone:
                _settings!.LastPortalRecall = value;
                break;
            case PluginRecallKind.Marketplace:
                _settings!.LastSecondaryRecall = value;
                break;
            case PluginRecallKind.Allegiance:
                _settings!.LastAllegianceRecall = value;
                break;
            case PluginRecallKind.House:
                _settings!.LastHouseRecall = value;
                break;
            case PluginRecallKind.Mansion:
                _settings!.LastMansionRecall = value;
                break;
        }
        if (
            _host.SessionSettings.TryGetValue("characterId", out string? characterId)
            && _host.SessionSettings.TryGetValue("worldId", out string? worldId)
            && !string.IsNullOrWhiteSpace(characterId)
            && !string.IsNullOrWhiteSpace(worldId)
        )
        {
            string key = $"{characterId}/{worldId}";
            if (!_settings!.RecallsByCharacter.TryGetValue(key, out var recalls))
                _settings.RecallsByCharacter[key] = recalls =
                    new GoArrowSettings.CharacterRecalls();
            switch (known.Kind)
            {
                case PluginRecallKind.Lifestone:
                    recalls.Lifestone = value;
                    break;
                case PluginRecallKind.Marketplace:
                    recalls.Marketplace = value;
                    break;
                case PluginRecallKind.Allegiance:
                    recalls.Allegiance = value;
                    break;
                case PluginRecallKind.House:
                    recalls.House = value;
                    break;
                case PluginRecallKind.Mansion:
                    recalls.Mansion = value;
                    break;
            }
        }
        _settings?.Save(_host.Storage);
    }

    private void LoadScopedRecallState(IPluginHost host)
    {
        if (
            _settings is null
            || !host.SessionSettings.TryGetValue("characterId", out string? characterId)
            || !host.SessionSettings.TryGetValue("worldId", out string? worldId)
            || string.IsNullOrWhiteSpace(characterId)
            || string.IsNullOrWhiteSpace(worldId)
        )
            return;
        string key = $"{characterId}/{worldId}";
        if (!_settings.RecallsByCharacter.TryGetValue(key, out var recalls))
        {
            IPluginStorage scoped = host
                .Storage.OpenScope(PluginStorageScope.Character(characterId))
                .OpenScope(PluginStorageScope.World(worldId));
            string? lifestone = scoped.ReadText("recall/lifestone");
            string? marketplace = scoped.ReadText("recall/marketplace");
            string? allegiance = scoped.ReadText("recall/allegiance");
            string? house = scoped.ReadText("recall/house");
            string? mansion = scoped.ReadText("recall/mansion");
            if (
                lifestone is not null
                || marketplace is not null
                || allegiance is not null
                || house is not null
                || mansion is not null
            )
            {
                recalls = new GoArrowSettings.CharacterRecalls
                {
                    Lifestone = lifestone ?? _settings.LastPortalRecall,
                    Marketplace = marketplace ?? _settings.LastSecondaryRecall,
                    Allegiance = allegiance ?? _settings.LastAllegianceRecall,
                    House = house ?? _settings.LastHouseRecall,
                    Mansion = mansion ?? _settings.LastMansionRecall,
                };
                _settings.RecallsByCharacter[key] = recalls;
                _settings.Save(host.Storage);
                scoped.Delete("recall/lifestone");
                scoped.Delete("recall/marketplace");
                scoped.Delete("recall/allegiance");
                scoped.Delete("recall/house");
                scoped.Delete("recall/mansion");
            }
        }
        if (recalls is null)
            return;
        _settings.LastPortalRecall = recalls.Lifestone;
        _settings.LastSecondaryRecall = recalls.Marketplace;
        _settings.LastAllegianceRecall = recalls.Allegiance;
        _settings.LastHouseRecall = recalls.House;
        _settings.LastMansionRecall = recalls.Mansion;
    }

    internal bool SetSelectedObjectDestination()
    {
        if (_host is null || !_host.Automation.IsAvailable || _destination is null)
            return false;
        uint? selected = _host.Selection.SelectedObjectId;
        if (
            selected is not { } objectId
            || !_host.Automation.Objects.TryGet(objectId, out PluginWorldObject obj)
            || !_destination.SetObject(obj)
        )
            return false;
        _navigator?.StopNavigation();
        _settings?.Save(_host.Storage);
        return true;
    }

    /// <summary>Save the current indoor point as a named, searchable location.</summary>
    internal bool MarkCurrentIndoorLocation(string name)
    {
        if (
            _host is null
            || _database is null
            || _destination is null
            || !_host.Storage.IsAvailable
            || string.IsNullOrWhiteSpace(name)
        )
            return false;
        var snapshot = _host.Automation.Navigation.Snapshot;
        var position = snapshot.Position;
        if (
            !snapshot.IsAvailable
            || snapshot.IsPortalSpace
            || position.IsOutdoor
            || (position.CellId & 0xFFFFu) <= 0x40u
        )
            return false;

        string trimmedName = name.Trim();
        var location = new Location(trimmedName, position.NorthSouth, position.EastWest)
        {
            IndoorPosition = position,
            IsCustomized = true,
            UseInRouteFinding = false,
        };
        try
        {
            var document = new XmlDocument();
            XmlElement root = document.CreateElement("locations");
            document.AppendChild(root);
            foreach (
                Location item in _database
                    .UserLocations.Where(item =>
                        !item.Name.Equals(trimmedName, StringComparison.OrdinalIgnoreCase)
                    )
                    .Append(location)
            )
            {
                var entry = new XmlDocument();
                entry.LoadXml(item.ToXml());
                root.AppendChild(document.ImportNode(entry.DocumentElement!, true));
            }
            _host.Storage.WriteText(IndoorLocationsStorageKey, document.OuterXml);
            _database.UpsertUserLocation(location);
            _routeFinder?.InvalidateGraph();
            _navigator?.StopNavigation();
            _destination.SetDestination(location);
            _settings?.Save(_host.Storage);
            return true;
        }
        catch (Exception exception)
        {
            _host.Log.Warn($"GoArrow: Could not save indoor location: {exception.Message}");
            return false;
        }
    }

    private void LoadSavedIndoorLocations()
    {
        if (_host is null || _database is null)
            return;
        string? xml = _host.Storage.ReadText(IndoorLocationsStorageKey);
        if (string.IsNullOrWhiteSpace(xml))
            return;
        try
        {
            var saved = new LocationDatabase();
            saved.LoadLocationsXml(xml);
            foreach (
                Location location in saved.AllLocations.Where(location =>
                    location.IndoorPosition is not null
                )
            )
                _database.UpsertUserLocation(location);
        }
        catch (Exception exception)
        {
            _host.Log.Warn(
                $"GoArrow: Ignoring invalid saved indoor locations: {exception.Message}"
            );
        }
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
        if (
            _destination?.Kind == GoArrowDestinationKind.Object
            && _destination.TargetObjectId is { } target
            && change.SelectedObjectId != target
        )
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

    internal bool DungeonMapVisible => _settings?.DungeonMapVisible ?? false;

    internal async Task UpdateDungeonMapsAsync()
    {
        if (_host is null || _settings is null)
            return;
        if (string.IsNullOrWhiteSpace(_settings.DungeonMapUrl))
        {
            DungeonDownloadStatus = "Set a dungeon map URL first.";
            _host.Automation.Chat.PostSystemMessage(
                "GoArrow: Set a dungeon map URL in Config before downloading."
            );
            return;
        }
        if (Interlocked.Exchange(ref _dungeonUpdateInProgress, 1) != 0)
        {
            _host.Automation.Chat.PostSystemMessage(
                "GoArrow: A dungeon map download is already running."
            );
            return;
        }
        try
        {
            DungeonDownloadStatus = "Downloading dungeon maps...";
            _host.Automation.Chat.PostSystemMessage("GoArrow: Downloading dungeon maps...");
            int count = await new DungeonMapDownloader(_host.Storage)
                .DownloadAsync(_settings.DungeonMapUrl)
                .ConfigureAwait(false);
            _downloadedDungeonMapCount = count;
            DungeonDownloadStatus = $"Downloaded {count} maps; loading...";
            Interlocked.Exchange(ref _pendingDungeonMapReload, 1);
        }
        catch (Exception exception)
        {
            _host.Log.Error("GoArrow: Dungeon map download failed.", exception);
            DungeonDownloadStatus = "Download failed; existing maps kept.";
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Dungeon map download failed: {exception.Message}"
            );
        }
        finally
        {
            Volatile.Write(ref _dungeonUpdateInProgress, 0);
        }
    }

    internal string? DungeonMapDirectory =>
        _host is null ? null : DungeonMapCatalog.UserMapDirectory(_host.Storage);

    internal bool ReloadDungeonMaps()
    {
        _dungeonMap?.Dispose();
        _dungeonMap = null;
        if (_host is null || _settings is null || !_host.HasUi)
            return false;

        try
        {
            DungeonMapCatalog? catalog = DungeonMapCatalog.OpenUserMaps(_host.Storage);
            if (catalog is null)
                return false;
            _dungeonMap = new GoArrowDungeonMap(_host, _settings, catalog);
            _dungeonMap.Enable();
            _host.Log.Info($"GoArrow: Loaded {catalog.Count} user dungeon maps.");
            return true;
        }
        catch (Exception exception)
        {
            _dungeonMap?.Dispose();
            _dungeonMap = null;
            _host.Log.Warn($"GoArrow: Dungeon maps unavailable: {exception.Message}");
            return false;
        }
    }

    internal void SetDungeonMapVisible(bool visible)
    {
        if (visible && _dungeonMap is null)
            ReloadDungeonMaps();
        _dungeonMap?.SetVisible(visible);
        if (_dungeonMap is null && _settings is not null && _host is not null)
        {
            _settings.DungeonMapVisible = visible;
            _settings.Save(_host.Storage);
        }
    }

    private void OnTick(double elapsed)
    {
        if (
            _pendingRouteWork != PendingRouteWork.None
            && Stopwatch.GetTimestamp() >= _routeWorkReadyAt
        )
        {
            var work = _pendingRouteWork;
            _pendingRouteWork = PendingRouteWork.None;
            _routeWorkInProgress = true;
            try
            {
                if (work == PendingRouteWork.Go)
                    ExecuteGo();
                else if (work == PendingRouteWork.Resume)
                    _navigator?.ResumeNavigation();
                else if (
                    _destination?.HasDestination == true
                    && _host?.Automation.IsAvailable == true
                )
                {
                    var preview = _host.Automation.Navigation.Snapshot;
                    if (preview.IsAvailable && !preview.IsPortalSpace && preview.Position.IsOutdoor)
                    {
                        var current = new Location(
                            "Current Position",
                            preview.Position.NorthSouth,
                            preview.Position.EastWest
                        );
                        _destination.CalculateRoute(_routeFromOverride ?? current);
                        _lastPreviewPosition = preview.Position;
                        _lastPreviewAt = Stopwatch.GetTimestamp();
                    }
                }
            }
            finally
            {
                _routeWorkInProgress = false;
            }
        }
        if (Interlocked.Exchange(ref _pendingDungeonMapReload, 0) != 0)
        {
            bool loaded = ReloadDungeonMaps();
            DungeonDownloadStatus = loaded
                ? $"Loaded {_downloadedDungeonMapCount} dungeon maps."
                : "Downloaded maps; enable UI to display them.";
            _host?.Automation.Chat.PostSystemMessage($"GoArrow: {DungeonDownloadStatus}");
        }
        if (_host == null || _destination == null || _navigator == null)
            return;

        // Update current position from navigation snapshot
        if (_host.Automation.IsAvailable)
        {
            var snapshot = _host.Automation.Navigation.Snapshot;
            if (
                _settings?.DestinationName == "Current Location"
                && !_destination.HasDestination
                && snapshot.IsAvailable
            )
                SetCurrentLocationDestination();
            var position = snapshot.Position;
            _navigator.UpdatePosition(position);
            if (snapshot.IsAvailable && (snapshot.IsPortalSpace || !position.IsOutdoor))
                _lastPreviewAt = 0;

            // Recalculate while idle. During navigation the navigator owns
            // the current route and advances it from navigation reports.
            if (
                _pendingRouteWork == PendingRouteWork.None
                && _destination.HasDestination
                && snapshot.IsAvailable
                && !snapshot.IsPortalSpace
                && position.IsOutdoor
                && !_navigator.IsNavigating
                && !_navigator.WaitingForInteraction
                && !_navigator.HasArrived
                && _navigator.FailureReason.Length == 0
                && (_destination.CurrentRoute is null || _settings?.RecalculateRoute == true)
                && (
                    _lastPreviewAt == 0
                    || Stopwatch.GetElapsedTime(_lastPreviewAt).TotalSeconds >= 1
                )
                && (
                    _destination.CurrentRoute is null
                    || _lastPreviewPosition is not { } last
                    || position.HorizontalDistanceMeters(last) >= 20
                )
            )
            {
                if (_host.HasUi)
                    QueueRouteWork(PendingRouteWork.Preview);
                else
                {
                    var currentLoc = new RouteFinding.Location(
                        "Current Position",
                        position.NorthSouth,
                        position.EastWest
                    );
                    _destination.CalculateRoute(_routeFromOverride ?? currentLoc);
                    _lastPreviewPosition = position;
                    _lastPreviewAt = Stopwatch.GetTimestamp();
                }
            }
            else if (
                _destination.HasDestination
                && snapshot.IsAvailable
                && !snapshot.IsPortalSpace
                && position.IsOutdoor
            )
            {
                var currentLoc = new RouteFinding.Location(
                    "Current Position",
                    position.NorthSouth,
                    position.EastWest
                );
                _destination.UpdateGuidance(_routeFromOverride ?? currentLoc);
            }
        }

        // Tick navigator
        _navigator.OnTick(elapsed);
        _panel?.OnTick(elapsed);
    }

    private void LoadCachedAtlasData()
    {
        if (_host is null || _database is null || _atlasProvider is null)
            return;

        try
        {
            string? cached = _atlasProvider.ReadCached();
            if (cached is null)
                return;
            _database.LoadLocationsXml(cached);
            _host.Log.Info($"GoArrow: Loaded {_database.LocationCount} cached Atlas locations.");
        }
        catch (Exception exception)
        {
            _host.Log.Warn($"GoArrow: Ignoring invalid cached Atlas data: {exception.Message}");
        }
    }

    private void LoadLayeredData()
    {
        if (_host is null || _database is null)
            return;
        // The catalog returns the host's deterministic embedded/installed/user
        // precedence order; later layers override earlier records.
        foreach (
            string resourceId in _host
                .Resources.ListDataFiles("data")
                .Where(id => id.EndsWith(".xml", StringComparison.OrdinalIgnoreCase))
        )
        {
            try
            {
                using Stream? stream = _host.Resources.OpenRead(resourceId);
                if (stream is null)
                    continue;
                using var reader = new StreamReader(stream);
                string xml = reader.ReadToEnd();
                var candidate = new LocationDatabase();
                candidate.LoadLocationsXml(xml);
                if (candidate.LocationCount == 0)
                    continue;
                _database.LoadLocationsXml(xml);
                _host.Log.Info(
                    $"GoArrow: Loaded layered data '{resourceId}' ({candidate.LocationCount} locations)."
                );
            }
            catch (Exception exception)
            {
                _host.Log.Warn(
                    $"GoArrow: Ignoring invalid layered data '{resourceId}': {exception.Message}"
                );
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
