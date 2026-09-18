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
        var args = string.IsNullOrEmpty(cmd.Arguments)
            ? Array.Empty<string>()
            : cmd.Arguments.Split(' ', StringSplitOptions.RemoveEmptyEntries);

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

            case "status":
                ShowStatus();
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
                _plugin.SetDestination("Recall");
                _host.Automation.Chat.PostSystemMessage("GoArrow: Navigating to recall location (use chat-based tracking).");
                break;

            default:
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

    private void ShowStatus()
    {
        var dest = _plugin.CurrentDestinationName;
        if (string.IsNullOrEmpty(dest))
            _host.Automation.Chat.PostSystemMessage("GoArrow: No destination set. Use /go <name>");
        else
            _host.Automation.Chat.PostSystemMessage($"GoArrow: Navigating to '{dest}'. Use /go stop to cancel.");
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
               "  /go file filename.xml - Load XML from the GoArrow storage directory\n" +
               "  /go update [url] - Download location data, optionally changing the URL\n" +
               "  /go url <url> - Set and persist the location-data URL\n" +
               "  /go search <term> - Search locations\n" +
               "  /go status - Show current destination\n" +
               "  /go stop - Stop navigation\n" +
               "  /go clear - Clear destination\n" +
               "  /go save <name> - Save location as favorite\n" +
               "  /go favorites - List favorites\n" +
               "  /go help - Show this help";
    }
}