using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow;

/// <summary>
/// Handles chat commands for the GoArrow plugin.
/// Commands are registered via IPluginCommandRegistry as "/go" sub-commands.
/// OpenAC uses "/" prefix; the original "@" prefix is not supported.
/// </summary>
internal sealed class GoArrowCommands
{
    private readonly IPluginHost _host;
    private readonly GoArrowPlugin _plugin;

    public GoArrowCommands(IPluginHost host, GoArrowPlugin plugin)
    {
        _host = host;
        _plugin = plugin;
    }

    /// <summary>
    /// Handle a "/go" command via PluginCommand (OpenAC command registry).
    /// </summary>
    public void HandleCommand(PluginCommand cmd)
    {
        string[] args;
        try
        {
            args = cmd.ParseArguments().ToArray();
        }
        catch (FormatException exception)
        {
            _host.Automation.Chat.PostSystemMessage($"GoArrow: {exception.Message}");
            return;
        }

        if (args.Length == 0)
        {
            _host.Automation.Chat.PostSystemMessage(HelpText());
            return;
        }

        var subCommand = args[0].ToLowerInvariant();
        switch (subCommand)
        {
            case "help":
            case "?":
                _host.Automation.Chat.PostSystemMessage(HelpText());
                break;

            case "list":
                ListDestinations();
                break;

            case "search":
            case "find":
                SearchDestinations(args.Length > 1 ? string.Join(" ", args.Skip(1)) : string.Empty);
                break;

            case "to":
                SetExplicitDestination(args.Skip(1));
                break;

            case "from":
            case "start":
                _host.Automation.Chat.PostSystemMessage(_plugin.CurrentPositionText("GoArrow: Route origin"));
                break;

            case "end":
                _plugin.StopNavigation();
                _host.Automation.Chat.PostSystemMessage("GoArrow: Current route leg ended; use /go resume to continue.");
                break;

            case "reset":
                _plugin.ClearDestination();
                _host.Automation.Chat.PostSystemMessage("GoArrow: Route reset.");
                break;

            case "lock":
                _plugin.SetNavigationLock(true);
                _host.Automation.Chat.PostSystemMessage("GoArrow: Navigation locked.");
                break;

            case "unlock":
                _plugin.SetNavigationLock(false);
                _host.Automation.Chat.PostSystemMessage("GoArrow: Navigation unlocked.");
                break;

            case "selected":
            case "attach":
                if (_plugin.SetSelectedObjectDestination())
                    _host.Automation.Chat.PostSystemMessage("GoArrow: Selected object attached as destination.");
                else
                    _host.Automation.Chat.PostSystemMessage("GoArrow: Selected object is unavailable or has no position.");
                break;

            case "mark":
                string markName = string.Join(" ", args.Skip(1)).Trim();
                _host.Automation.Chat.PostSystemMessage(_plugin.MarkCurrentIndoorLocation(markName)
                    ? $"GoArrow: Saved indoor location '{markName}'."
                    : "GoArrow: Stand at an indoor point and use /go mark <name>.");
                break;

            case "loc":
                _host.Automation.Chat.PostSystemMessage(_plugin.CurrentPositionText());
                break;

            case "dest":
                _host.Automation.Chat.PostSystemMessage(_plugin.DestinationPositionText());
                break;

            case "file":
                if (args.Length != 2 || !_plugin.LoadDataFile(args[1]))
                {
                    _host.Automation.Chat.PostSystemMessage(
                        "GoArrow: Usage: /go file filename.xml (file not found or invalid).");
                    break;
                }
                _host.Automation.Chat.PostSystemMessage(
                    $"GoArrow: Loaded location data from '{args[1]}'.");
                break;

            case "update":
            case "download":
                if (args.Length > 1 && !_plugin.SetExternalDataUrl(args[1]))
                {
                    _host.Automation.Chat.PostSystemMessage(
                        "GoArrow: URL must be an absolute http:// or https:// URL.");
                    break;
                }
                _ = _plugin.UpdateDataAsync();
                break;

            case "url":
                if (args.Length != 2 || !_plugin.SetExternalDataUrl(args[1]))
                {
                    _host.Automation.Chat.PostSystemMessage(
                        "GoArrow: Usage: /go url <http:// or https:// URL>");
                    break;
                }
                _host.Automation.Chat.PostSystemMessage(
                    $"GoArrow: Location-data URL set to '{args[1]}'.");
                break;

            case "stop":
            case "cancel":
                _plugin.StopNavigation();
                _host.Automation.Chat.PostSystemMessage("GoArrow: Navigation stopped.");
                break;

            case "resume":
                _plugin.ResumeNavigation();
                break;

            case "status":
                ShowStatus();
                break;

            case "route":
            case "steps":
                ShowRoute();
                break;

            case "dungeon":
                string mode = args.Length > 1 ? args[1].ToLowerInvariant() : "toggle";
                if (args.Length > 2 || mode is not ("on" or "off" or "toggle" or "path" or "reload"))
                {
                    _host.Automation.Chat.PostSystemMessage("GoArrow: Usage: /go dungeon [on|off|toggle|path|reload]");
                    break;
                }
                if (mode == "path")
                {
                    _host.Automation.Chat.PostSystemMessage(
                        $"GoArrow: Dungeon map folder: {_plugin.DungeonMapDirectory ?? "unavailable"}");
                    break;
                }
                if (mode == "reload")
                {
                    _host.Automation.Chat.PostSystemMessage(_plugin.ReloadDungeonMaps()
                        ? "GoArrow: Dungeon maps reloaded."
                        : "GoArrow: No usable dungeon maps found. Use /go dungeon path for the folder.");
                    break;
                }
                bool visible = mode == "toggle"
                    ? !_plugin.DungeonMapVisible
                    : mode == "on";
                _plugin.SetDungeonMapVisible(visible);
                _host.Automation.Chat.PostSystemMessage(
                    $"GoArrow: Dungeon map {(visible ? "enabled" : "disabled")}.");
                break;

            case "favorites":
            case "favs":
                ListFavorites();
                break;

            case "save":
                if (args.Length > 1)
                    SaveFavorite(args[1]);
                else
                    _host.Automation.Chat.PostSystemMessage("GoArrow: Usage: /go save <name>");
                break;

            case "clear":
                _plugin.ClearDestination();
                _host.Automation.Chat.PostSystemMessage("GoArrow: Destination cleared.");
                break;

            case "recall":
                _plugin.Recall(args.Length > 1 ? args[1] : "lifestone");
                break;

            default:
                // Coordinates are accepted as the unquoted shorthand /go 42.1N 33.6E.
                if (_plugin.TrySetCoordinateDestination(string.Join(" ", args)))
                {
                    _host.Automation.Chat.PostSystemMessage("GoArrow: Coordinate destination set.");
                    break;
                }

                // Treat as a destination name (may be multi-word)
                var destName = string.Join(" ", args).Trim();
                if (!string.IsNullOrEmpty(destName))
                {
                    if (_plugin.SetDestination(destName))
                        _host.Automation.Chat.PostSystemMessage($"GoArrow: Destination set to '{destName}'.");
                    else
                        _host.Automation.Chat.PostSystemMessage($"GoArrow: Destination '{destName}' not found in location database.");
                }
                break;
        }
    }

    private void SetExplicitDestination(IEnumerable<string> values)
    {
        string text = string.Join(" ", values).Trim();
        if (string.IsNullOrWhiteSpace(text))
        {
            _host.Automation.Chat.PostSystemMessage("GoArrow: Usage: /go to <location|coordinates|here>");
            return;
        }
        if (text.Equals("here", StringComparison.OrdinalIgnoreCase))
        {
            _host.Automation.Chat.PostSystemMessage(_plugin.CurrentPositionText("GoArrow: Current position"));
            return;
        }
        if (_plugin.TrySetCoordinateDestination(text) || _plugin.SetDestination(text))
            _host.Automation.Chat.PostSystemMessage("GoArrow: Destination set.");
        else
            _host.Automation.Chat.PostSystemMessage($"GoArrow: Destination '{text}' not found.");
    }

    private void ListDestinations()
    {
        var names = _plugin.GetAllDestinationNames();
        if (names.Count == 0)
        {
            _host.Automation.Chat.PostSystemMessage("GoArrow: No locations loaded.");
            return;
        }

        var message = "GoArrow Locations: " + string.Join(", ", names.Take(20));
        _host.Automation.Chat.PostSystemMessage(message);
        if (names.Count > 20)
            _host.Automation.Chat.PostSystemMessage($"... and {names.Count - 20} more.");
    }

    private void SearchDestinations(string query)
    {
        if (string.IsNullOrWhiteSpace(query))
        {
            _host.Automation.Chat.PostSystemMessage("GoArrow: Usage: /go search <term>");
            return;
        }

        var matches = _plugin.SearchLocations(query);
        if (matches.Count == 0)
        {
            _host.Automation.Chat.PostSystemMessage($"GoArrow: No locations match '{query}'.");
            return;
        }

        _host.Automation.Chat.PostSystemMessage(
            "GoArrow matches: " + string.Join(", ", matches.Take(20).Select(location => location.Name)));
        if (matches.Count > 20)
            _host.Automation.Chat.PostSystemMessage($"... and {matches.Count - 20} more.");
    }

    private void ShowStatus()
    {
        var dest = _plugin.CurrentDestinationName;
        var snapshot = _host.Automation.Navigation.Snapshot;
        string place = !snapshot.IsAvailable ? "position unavailable"
            : snapshot.IsPortalSpace ? "portal space"
            : snapshot.Position.IsOutdoor ? "outdoors" : "indoors";
        string step = _plugin.GetCurrentRouteSteps().FirstOrDefault() ?? "no active step";
        _host.Automation.Chat.PostSystemMessage(
            $"{GoArrowPlugin.DisplayTitle}: {_plugin.Panel?.NavStatusText ?? "Idle"}; {place}; "
            + $"client {_host.Automation.Navigation.GoToReport.State}; next {step}.");
        if (_plugin.NavigationFailureReason.Length > 0)
            _host.Automation.Chat.PostSystemMessage(
                $"GoArrow: Reason: {_plugin.NavigationFailureReason}");
        if (_host.Automation.Recalls.IsAvailable)
        {
            int known = _host.Automation.Recalls.CaptureLocations().Count(location => location.IsKnown);
            _host.Automation.Chat.PostSystemMessage($"GoArrow: Recall destinations known: {known}.");
        }
        if (string.IsNullOrEmpty(dest))
            _host.Automation.Chat.PostSystemMessage("GoArrow: No destination set. Use /go <name>");
        else
            _host.Automation.Chat.PostSystemMessage($"GoArrow: Destination '{dest}'. Use /go stop to cancel.");
    }

    private void ShowRoute()
    {
        var steps = _plugin.GetCurrentRouteSteps();
        if (steps.Count == 0)
        {
            _host.Automation.Chat.PostSystemMessage("GoArrow: No route is currently calculated.");
            return;
        }

        _host.Automation.Chat.PostSystemMessage("GoArrow route:");
        for (int i = 0; i < steps.Count; i++)
            _host.Automation.Chat.PostSystemMessage($"  {i + 1}. {steps[i]}");
    }

    private void ListFavorites()
    {
        var favs = _plugin.GetFavorites();
        if (favs.Count == 0)
        {
            _host.Automation.Chat.PostSystemMessage("GoArrow: No favorites saved. Use /go save <name>");
            return;
        }
        _host.Automation.Chat.PostSystemMessage("GoArrow Favorites: " + string.Join(", ", favs));
    }

    private void SaveFavorite(string name)
    {
        _plugin.AddFavorite(name);
        _host.Automation.Chat.PostSystemMessage($"GoArrow: '{name}' saved to favorites.");
    }

    private static string HelpText()
    {
        return "GoArrow Commands:\n" +
               "  /go <destination> - Set route to a named location\n" +
               "  /go list - List all known locations\n" +
               "  /go search <term> - Search locations\n" +
               "  /go loc - Show current coordinates\n" +
               "  /go dest - Show destination coordinates\n" +
               "  /go file filename.xml - Load XML from the GoArrow storage directory\n" +
               "  /go update [url] - Download location data, optionally changing the URL\n" +
               "  /go url <url> - Set and persist the location-data URL\n" +
               "  /go search <term> - Search locations\n" +
               "  /go status - Show current destination\n" +
               "  /go route - Show the current route steps\n" +
               "  /go dungeon [on|off|toggle|path|reload] - Manage user dungeon maps\n" +
               "  /go mark <name> - Save this indoor point as a named location\n" +
               "  /go stop - Stop navigation\n" +
               "  /go resume - Resume after a portal or recall interaction\n" +
               "  /go clear - Clear destination\n" +
               "  /go save <name> - Save location as favorite\n" +
               "  /go favorites - List favorites\n" +
               "  /go help - Show this help";
    }
}
