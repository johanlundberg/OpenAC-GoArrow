using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow;

/// <summary>Typed command metadata and non-blocking completion for /go.</summary>
internal sealed class GoArrowCommandDefinition : IPluginCommandDefinition
{
    private static readonly string[] Subcommands =
    {
        "to", "from", "start", "end", "selected", "attach", "list", "search",
        "loc", "dest", "mark", "route", "status", "stop", "resume", "clear", "reset",
        "lock", "unlock", "recall", "save", "favorites", "update", "help"
    };
    private readonly GoArrowCommands _commands;
    private readonly GoArrowPlugin _plugin;

    public GoArrowCommandDefinition(GoArrowCommands commands, GoArrowPlugin plugin)
    {
        _commands = commands;
        _plugin = plugin;
    }

    public string Verb => "go";
    public string Description => "Set and control GoArrow destinations and navigation.";
    public IReadOnlyList<string> Aliases => new[] { "goarrow" };
    public PluginCommandResult Invoke(PluginCommand command)
    {
        _commands.HandleCommand(command);
        return PluginCommandResult.Accepted();
    }

    public IReadOnlyList<PluginCommandCompletion> Complete(PluginCommand command)
    {
        string[] args;
        try { args = command.ParseArguments().ToArray(); }
        catch (FormatException) { return Array.Empty<PluginCommandCompletion>(); }
        string prefix = args.Length == 0 ? string.Empty : args[^1];
        if (args.Length <= 1)
            return Subcommands.Where(value => value.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                .Select(value => new PluginCommandCompletion(value)).ToArray();
        if (args[0].Equals("to", StringComparison.OrdinalIgnoreCase)
            || args[0].Equals("search", StringComparison.OrdinalIgnoreCase))
            return _plugin.SearchLocations(prefix).Take(20)
                .Select(location => new PluginCommandCompletion(location.Name, location.Notes)).ToArray();
        return Array.Empty<PluginCommandCompletion>();
    }
}
