using System.Text.Json;

namespace AcDream.Plugins.GoArrow.RouteFinding;

/// <summary>
/// Corrections to route data: Atlas portal arrivals by Atlas id, and route
/// steps that route searches must not use. A wrong portal arrival makes the
/// graph walk on from a place the portal never reaches, so navigation
/// re-plans the same portal after every arrival.
/// </summary>
public sealed class AtlasCorrections
{
    public int Version { get; set; } = 1;
    public List<ArrivalCorrection> Arrivals { get; set; } = [];
    public List<BlockedStep> BlockedSteps { get; set; } = [];

    public sealed class ArrivalCorrection
    {
        /// <summary>Atlas location id; the only field used for matching.</summary>
        public int Id { get; set; }

        /// <summary>Atlas name, for people reading the file.</summary>
        public string Name { get; set; } = "";

        /// <summary>Correct arrival in map notation, such as "52.2S, 82.5E".</summary>
        public string Arrival { get; set; } = "";

        public string Note { get; set; } = "";
    }

    /// <summary>
    /// A route step that does not work in the game. Each end matches a graph
    /// location by Atlas id when the id is positive, otherwise by name. A
    /// blocked walk is blocked in both directions.
    /// </summary>
    public sealed class BlockedStep
    {
        /// <summary>Travel, Portal or Recall, as in the route step list.</summary>
        public string Kind { get; set; } = "";
        public string From { get; set; } = "";
        public int FromId { get; set; }
        public string To { get; set; } = "";
        public int ToId { get; set; }
        public string Note { get; set; } = "";

        internal static BlockedStep FromRoute(RouteStep step, string note) => new()
        {
            Kind = step.Kind.ToString(),
            From = step.GraphFrom.Name,
            FromId = step.GraphFrom.Id,
            To = step.To.Name,
            ToId = step.To.Id,
            Note = note,
        };

        internal bool Blocks(Location from, Location to, RouteEdgeKind kind)
        {
            bool sameKind = kind switch
            {
                RouteEdgeKind.Walk => Kind.Equals("Travel", StringComparison.OrdinalIgnoreCase),
                RouteEdgeKind.Portal => Kind.Equals("Portal", StringComparison.OrdinalIgnoreCase),
                _ => Kind.Equals("Recall", StringComparison.OrdinalIgnoreCase),
            };
            if (!sameKind)
                return false;
            if (Matches(from, From, FromId) && Matches(to, To, ToId))
                return true;
            return kind == RouteEdgeKind.Walk && Matches(to, From, FromId) && Matches(from, To, ToId);
        }

        private static bool Matches(Location location, string name, int id) =>
            id > 0 ? location.Id == id : location.Name.Equals(name, StringComparison.OrdinalIgnoreCase);
    }

    private static readonly JsonSerializerOptions Options = new()
    {
        PropertyNameCaseInsensitive = true,
        ReadCommentHandling = JsonCommentHandling.Skip,
        AllowTrailingCommas = true,
    };

    public static AtlasCorrections Parse(string json) =>
        JsonSerializer.Deserialize<AtlasCorrections>(json, Options) ?? new();

    public string ToJson() =>
        JsonSerializer.Serialize(this, new JsonSerializerOptions(Options) { WriteIndented = true });

    /// <summary>
    /// Arrival coordinates by Atlas id. Later sources override earlier ones.
    /// An entry without an id or readable arrival is reported, not applied.
    /// </summary>
    public static Dictionary<int, Coordinates> ArrivalsById(
        IEnumerable<AtlasCorrections> sources,
        ICollection<string> errors
    )
    {
        var result = new Dictionary<int, Coordinates>();
        foreach (var source in sources)
        {
            foreach (var entry in source.Arrivals)
            {
                if (entry.Id <= 0)
                    errors.Add($"Arrival correction '{entry.Name}' has no Atlas id.");
                else if (!Coordinates.TryParse(entry.Arrival, out Coordinates arrival))
                    errors.Add($"Arrival correction {entry.Id} has unreadable arrival '{entry.Arrival}'.");
                else
                    result[entry.Id] = arrival;
            }
        }
        return result;
    }
}
