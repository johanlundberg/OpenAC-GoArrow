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
    private IDisposable? _commandRegistration;
    private Action<double>? _tickHandler;

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

        // ── Initialize database and load embedded data ──────────────
        _database = new LocationDatabase();
        LoadEmbeddedData();

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
        _commandRegistration = _host.Commands.Register(
            "go",
            cmd => _commands?.HandleCommand(cmd));

        // Subscribe to tick events
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

            // Recalculate route on tick if destination is set
            if (_destination.HasDestination)
            {
                var currentLoc = new RouteFinding.Location(
                    "Current Position",
                    position.NorthSouth,
                    position.EastWest);
                _destination.CalculateRoute(currentLoc);
            }
        }

        // Tick navigator
        _navigator.OnTick(elapsed);
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