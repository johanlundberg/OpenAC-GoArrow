using System.Globalization;
using System.Text.Json;
using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow;

/// <summary>
/// Persistent settings for GoArrow, stored in one JSON document.
/// </summary>
public class GoArrowSettings
{
    private static readonly JsonSerializerOptions JsonOptions = new() { WriteIndented = true };
    private static readonly string[] LegacyKeys =
    [
        "destination",
        "autoNavigate",
        "recalculate",
        "panelVisible",
        "showDistance",
        "showBearing",
        "hudVisible",
        "toolbarVisible",
        "hudClickThrough",
        "hudScale",
        "arrowOffsetX",
        "arrowOffsetY",
        "toolbarOffsetX",
        "toolbarOffsetY",
        "dungeonOffsetX",
        "dungeonOffsetY",
        "overlayPositionVersion",
        "mapVisible",
        "dungeonMapVisible",
        "mapCenterEW",
        "mapCenterNS",
        "mapWidth",
        "mapHeight",
        "arrivalDistance",
        "useNavigation",
        "navigationLocked",
        "routeCostProfile",
        "maxNavigationRetries",
        "interactionTimeoutSeconds",
        "atlasCacheMaxAgeDays",
        "externalDataUrl",
        "dungeonMapUrl",
        "lastPortalRecall",
        "lastSecondaryRecall",
        "lastAllegianceRecall",
        "lastHouseRecall",
        "lastMansionRecall",
        "favorites",
    ];

    // ── Destination Tracking ───────────────────────────────────────
    public string DestinationName { get; set; } = string.Empty;
    public bool AutoNavigate { get; set; } = false;
    public bool RecalculateRoute { get; set; } = true;

    // ── Display ────────────────────────────────────────────────────
    public bool PanelVisible { get; set; } = true;
    public bool ShowDistance { get; set; } = true;
    public bool ShowBearing { get; set; } = true;
    public bool HudVisible { get; set; } = true;
    public bool ToolbarVisible { get; set; } = true;
    public int VisibilitySettingsVersion { get; set; }
    public bool HudClickThrough { get; set; }
    public double HudScale { get; set; } = 1;
    public double ArrowOffsetX { get; set; } = -70;
    public double ArrowOffsetY { get; set; } = 120;
    public double ToolbarOffsetX { get; set; } = -70;
    public double ToolbarOffsetY { get; set; } = 215;
    public double DungeonOffsetX { get; set; } = 25;
    public double DungeonOffsetY { get; set; } = 95;
    public int OverlayPositionVersion { get; set; }
    public bool MapVisible { get; set; } = true;
    public bool DungeonMapVisible { get; set; } = true;
    public double MapCenterEastWest { get; set; }
    public double MapCenterNorthSouth { get; set; }
    public double MapWidth { get; set; } = 1000;
    public double MapHeight { get; set; } = 1000;

    // ── Navigation ─────────────────────────────────────────────────
    public double ArrivalDistance { get; set; } = 0.5;
    public bool UseNavigationAutomation { get; set; } = true;
    public bool NavigationLocked { get; set; }
    public RouteFinding.RouteCostProfile RouteCostProfile { get; set; } =
        RouteFinding.RouteCostProfile.ShortestWalk;
    public int MaxNavigationRetries { get; set; } = 2;
    public double InteractionTimeoutSeconds { get; set; } = 15;
    public int AtlasCacheMaxAgeDays { get; set; } = 30;

    // ── External data ──────────────────────────────────────────────
    /// <summary>Location-data URL used by the explicit update command.</summary>
    public string ExternalDataUrl { get; set; } = string.Empty;
    public string DungeonMapUrl { get; set; } = string.Empty;

    // ── Recall Tracking ────────────────────────────────────────────
    public string LastPortalRecall { get; set; } = string.Empty;
    public string LastSecondaryRecall { get; set; } = string.Empty;
    public string LastAllegianceRecall { get; set; } = string.Empty;
    public string LastHouseRecall { get; set; } = string.Empty;
    public string LastMansionRecall { get; set; } = string.Empty;
    public Dictionary<string, CharacterRecalls> RecallsByCharacter { get; set; } = new();

    public sealed class CharacterRecalls
    {
        public string Lifestone { get; set; } = string.Empty;
        public string Marketplace { get; set; } = string.Empty;
        public string Allegiance { get; set; } = string.Empty;
        public string House { get; set; } = string.Empty;
        public string Mansion { get; set; } = string.Empty;
    }

    // ── Favorites ──────────────────────────────────────────────────
    public List<string> FavoriteDestinations { get; set; } = new();

    public void Save(IPluginStorage storage)
    {
        OverlayPositionVersion = 1;
        VisibilitySettingsVersion = 1;
        storage.WriteJson("settings.json", this, JsonOptions);
        foreach (string key in LegacyKeys)
            storage.Delete(key);
    }

    public void Load(IPluginStorage storage)
    {
        GoArrowSettings? structured = null;
        try
        {
            structured = storage.ReadJson<GoArrowSettings>("settings.json");
        }
        catch (Exception)
        { /* malformed structured data falls back to legacy keys */
        }
        if (structured is not null)
        {
            DestinationName = structured.DestinationName;
            AutoNavigate = structured.AutoNavigate;
            RecalculateRoute = structured.RecalculateRoute;
            PanelVisible = structured.PanelVisible;
            ShowDistance = structured.ShowDistance;
            ShowBearing = structured.ShowBearing;
            HudVisible = structured.HudVisible;
            ToolbarVisible = structured.ToolbarVisible;
            VisibilitySettingsVersion = structured.VisibilitySettingsVersion;
            HudClickThrough = structured.HudClickThrough;
            HudScale = structured.HudScale > 0 ? structured.HudScale : 1;
            ArrowOffsetX = structured.ArrowOffsetX;
            ArrowOffsetY = structured.ArrowOffsetY;
            ToolbarOffsetX = structured.ToolbarOffsetX;
            ToolbarOffsetY = structured.ToolbarOffsetY;
            DungeonOffsetX = structured.DungeonOffsetX;
            DungeonOffsetY = structured.DungeonOffsetY;
            OverlayPositionVersion = structured.OverlayPositionVersion;
            if (OverlayPositionVersion < 1)
                ResetOverlayPositions();
            MapVisible = structured.MapVisible;
            DungeonMapVisible = structured.DungeonMapVisible;
            MapCenterEastWest = structured.MapCenterEastWest;
            MapCenterNorthSouth = structured.MapCenterNorthSouth;
            MapWidth = structured.MapWidth > 0 ? structured.MapWidth : 1000;
            MapHeight = structured.MapHeight > 0 ? structured.MapHeight : 1000;
            ArrivalDistance = structured.ArrivalDistance > 0 ? structured.ArrivalDistance : 0.5;
            UseNavigationAutomation = structured.UseNavigationAutomation;
            NavigationLocked = structured.NavigationLocked;
            RouteCostProfile = structured.RouteCostProfile;
            MaxNavigationRetries = Math.Max(0, structured.MaxNavigationRetries);
            InteractionTimeoutSeconds =
                structured.InteractionTimeoutSeconds > 0
                    ? structured.InteractionTimeoutSeconds
                    : 15;
            AtlasCacheMaxAgeDays = Math.Max(0, structured.AtlasCacheMaxAgeDays);
            ExternalDataUrl = structured.ExternalDataUrl ?? string.Empty;
            DungeonMapUrl = structured.DungeonMapUrl ?? string.Empty;
            LastPortalRecall = structured.LastPortalRecall;
            LastSecondaryRecall = structured.LastSecondaryRecall;
            LastAllegianceRecall = structured.LastAllegianceRecall;
            LastHouseRecall = structured.LastHouseRecall;
            LastMansionRecall = structured.LastMansionRecall;
            RecallsByCharacter = structured.RecallsByCharacter ?? new();
            FavoriteDestinations = structured.FavoriteDestinations ?? new List<string>();
            if (VisibilitySettingsVersion < 1)
                RestoreUiHiddenByOldDefaults();
            foreach (string key in LegacyKeys)
                storage.Delete(key);
            return;
        }

        DestinationName = storage.ReadText("destination") ?? string.Empty;

        AutoNavigate = ReadLegacyBoolean(storage, "autoNavigate", AutoNavigate);
        RecalculateRoute = ReadLegacyBoolean(storage, "recalculate", RecalculateRoute);
        PanelVisible = ReadLegacyBoolean(storage, "panelVisible", PanelVisible);
        ShowDistance = ReadLegacyBoolean(storage, "showDistance", ShowDistance);
        ShowBearing = ReadLegacyBoolean(storage, "showBearing", ShowBearing);
        HudVisible = ReadLegacyBoolean(storage, "hudVisible", HudVisible);
        ToolbarVisible = ReadLegacyBoolean(storage, "toolbarVisible", ToolbarVisible);
        HudClickThrough = ReadLegacyBoolean(storage, "hudClickThrough", HudClickThrough);
        if (
            double.TryParse(
                storage.ReadText("hudScale"),
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out double hudScale
            )
            && hudScale > 0
        )
            HudScale = hudScale;
        ArrowOffsetX = ReadLegacyDouble(storage, "arrowOffsetX", ArrowOffsetX);
        ArrowOffsetY = ReadLegacyDouble(storage, "arrowOffsetY", ArrowOffsetY);
        ToolbarOffsetX = ReadLegacyDouble(storage, "toolbarOffsetX", ToolbarOffsetX);
        ToolbarOffsetY = ReadLegacyDouble(storage, "toolbarOffsetY", ToolbarOffsetY);
        DungeonOffsetX = ReadLegacyDouble(storage, "dungeonOffsetX", DungeonOffsetX);
        DungeonOffsetY = ReadLegacyDouble(storage, "dungeonOffsetY", DungeonOffsetY);
        if (
            int.TryParse(storage.ReadText("overlayPositionVersion"), out int overlayPositionVersion)
        )
            OverlayPositionVersion = overlayPositionVersion;
        if (OverlayPositionVersion < 1)
            ResetOverlayPositions();
        MapVisible = ReadLegacyBoolean(storage, "mapVisible", MapVisible);
        DungeonMapVisible = ReadLegacyBoolean(storage, "dungeonMapVisible", DungeonMapVisible);
        if (
            double.TryParse(
                storage.ReadText("mapCenterEW"),
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out double mapEW
            )
        )
            MapCenterEastWest = mapEW;
        if (
            double.TryParse(
                storage.ReadText("mapCenterNS"),
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out double mapNS
            )
        )
            MapCenterNorthSouth = mapNS;
        if (
            double.TryParse(
                storage.ReadText("mapWidth"),
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out double mapWidth
            )
            && mapWidth > 0
        )
            MapWidth = mapWidth;
        if (
            double.TryParse(
                storage.ReadText("mapHeight"),
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out double mapHeight
            )
            && mapHeight > 0
        )
            MapHeight = mapHeight;

        double.TryParse(
            storage.ReadText("arrivalDistance"),
            NumberStyles.Float,
            CultureInfo.InvariantCulture,
            out double arrDist
        );
        ArrivalDistance = arrDist > 0 ? arrDist : 0.5;

        UseNavigationAutomation = ReadLegacyBoolean(
            storage,
            "useNavigation",
            UseNavigationAutomation
        );
        NavigationLocked = ReadLegacyBoolean(storage, "navigationLocked", NavigationLocked);
        if (
            Enum.TryParse(
                storage.ReadText("routeCostProfile"),
                true,
                out RouteFinding.RouteCostProfile profile
            )
        )
            RouteCostProfile = profile;
        if (int.TryParse(storage.ReadText("maxNavigationRetries"), out int retries))
            MaxNavigationRetries = Math.Max(0, retries);
        if (
            double.TryParse(
                storage.ReadText("interactionTimeoutSeconds"),
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out double timeout
            )
            && timeout > 0
        )
            InteractionTimeoutSeconds = timeout;
        if (int.TryParse(storage.ReadText("atlasCacheMaxAgeDays"), out int cacheDays))
            AtlasCacheMaxAgeDays = Math.Max(0, cacheDays);

        ExternalDataUrl = storage.ReadText("externalDataUrl") ?? string.Empty;
        DungeonMapUrl = storage.ReadText("dungeonMapUrl") ?? string.Empty;

        LastPortalRecall = storage.ReadText("lastPortalRecall") ?? string.Empty;
        LastSecondaryRecall = storage.ReadText("lastSecondaryRecall") ?? string.Empty;
        LastAllegianceRecall = storage.ReadText("lastAllegianceRecall") ?? string.Empty;
        LastHouseRecall = storage.ReadText("lastHouseRecall") ?? string.Empty;
        LastMansionRecall = storage.ReadText("lastMansionRecall") ?? string.Empty;

        var favs = storage.ReadText("favorites") ?? string.Empty;
        FavoriteDestinations = favs.Split(new[] { ',' }, StringSplitOptions.RemoveEmptyEntries)
            .ToList();
        RestoreUiHiddenByOldDefaults();
        if (storage.IsAvailable && LegacyKeys.Any(key => storage.ReadText(key) is not null))
            Save(storage);
    }

    private static bool ReadLegacyBoolean(IPluginStorage storage, string key, bool defaultValue) =>
        bool.TryParse(storage.ReadText(key), out bool value) ? value : defaultValue;

    private static double ReadLegacyDouble(
        IPluginStorage storage,
        string key,
        double defaultValue
    ) =>
        double.TryParse(
            storage.ReadText(key),
            NumberStyles.Float,
            CultureInfo.InvariantCulture,
            out double value
        ) && double.IsFinite(value)
            ? value
            : defaultValue;

    public void ResetOverlayPositions()
    {
        ArrowOffsetX = -70;
        ArrowOffsetY = 120;
        ToolbarOffsetX = -70;
        ToolbarOffsetY = 215;
        DungeonOffsetX = 25;
        DungeonOffsetY = 95;
        OverlayPositionVersion = 1;
    }

    private void RestoreUiHiddenByOldDefaults()
    {
        // Earlier builds interpreted missing legacy keys as false and saved all
        // hidden states to settings.json. Recover the panel and arrow when
        // that leaves every overlay hidden.
        if (!PanelVisible && !HudVisible && !ToolbarVisible && !MapVisible)
            PanelVisible = true;
        if (!HudVisible && !ToolbarVisible && !MapVisible)
        {
            if (!ShowDistance && !ShowBearing && !RecalculateRoute && !UseNavigationAutomation)
            {
                ShowDistance = true;
                ShowBearing = true;
                RecalculateRoute = true;
                UseNavigationAutomation = true;
            }
            HudVisible = true;
        }
    }
}
