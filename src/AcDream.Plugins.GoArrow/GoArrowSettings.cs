using System.Globalization;
using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow;

/// <summary>
/// Persistent settings for GoArrow, stored via IPluginStorage.
/// Uses ReadText/WriteText key-value API.
/// </summary>
public class GoArrowSettings
{
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
    public bool HudClickThrough { get; set; }
    public double HudScale { get; set; } = 1;
    public bool MapVisible { get; set; } = true;
    public double MapCenterEastWest { get; set; }
    public double MapCenterNorthSouth { get; set; }
    public double MapWidth { get; set; } = 1000;
    public double MapHeight { get; set; } = 1000;

    // ── Navigation ─────────────────────────────────────────────────
    public double ArrivalDistance { get; set; } = 0.5;
    public bool UseNavigationAutomation { get; set; } = true;
    public bool NavigationLocked { get; set; }
    public RouteFinding.RouteCostProfile RouteCostProfile { get; set; } = RouteFinding.RouteCostProfile.ShortestWalk;
    public int MaxNavigationRetries { get; set; } = 2;
    public double InteractionTimeoutSeconds { get; set; } = 15;
    public int AtlasCacheMaxAgeDays { get; set; } = 30;

    // ── External data ──────────────────────────────────────────────
    /// <summary>Location-data URL used by the explicit update command.</summary>
    public string ExternalDataUrl { get; set; } =
        RouteFinding.WarcryAtlasDataProvider.DefaultUrl;

    // ── Recall Tracking ────────────────────────────────────────────
    public string LastPortalRecall { get; set; } = string.Empty;
    public string LastSecondaryRecall { get; set; } = string.Empty;
    public string LastAllegianceRecall { get; set; } = string.Empty;
    public string LastHouseRecall { get; set; } = string.Empty;
    public string LastMansionRecall { get; set; } = string.Empty;

    // ── Favorites ──────────────────────────────────────────────────
    public List<string> FavoriteDestinations { get; set; } = new();

    public void Save(IPluginStorage storage)
    {
        // Keep the legacy keys for older installations while using one atomic
        // structured document for new hosts.
        storage.WriteJson("settings.json", this);
        storage.WriteText("destination", DestinationName);
        storage.WriteText("autoNavigate", AutoNavigate.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("recalculate", RecalculateRoute.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("panelVisible", PanelVisible.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("showDistance", ShowDistance.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("showBearing", ShowBearing.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("hudVisible", HudVisible.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("toolbarVisible", ToolbarVisible.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("hudClickThrough", HudClickThrough.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("hudScale", HudScale.ToString("F3", CultureInfo.InvariantCulture));
        storage.WriteText("mapVisible", MapVisible.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("mapCenterEW", MapCenterEastWest.ToString("F4", CultureInfo.InvariantCulture));
        storage.WriteText("mapCenterNS", MapCenterNorthSouth.ToString("F4", CultureInfo.InvariantCulture));
        storage.WriteText("mapWidth", MapWidth.ToString("F4", CultureInfo.InvariantCulture));
        storage.WriteText("mapHeight", MapHeight.ToString("F4", CultureInfo.InvariantCulture));
        storage.WriteText("arrivalDistance", ArrivalDistance.ToString("F4", CultureInfo.InvariantCulture));
        storage.WriteText("useNavigation", UseNavigationAutomation.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("navigationLocked", NavigationLocked.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("routeCostProfile", RouteCostProfile.ToString());
        storage.WriteText("maxNavigationRetries", MaxNavigationRetries.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("interactionTimeoutSeconds", InteractionTimeoutSeconds.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("atlasCacheMaxAgeDays", AtlasCacheMaxAgeDays.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("externalDataUrl", ExternalDataUrl);
        storage.WriteText("lastPortalRecall", LastPortalRecall);
        storage.WriteText("lastSecondaryRecall", LastSecondaryRecall);
        storage.WriteText("lastAllegianceRecall", LastAllegianceRecall);
        storage.WriteText("lastHouseRecall", LastHouseRecall);
        storage.WriteText("lastMansionRecall", LastMansionRecall);
        storage.WriteText("favorites", string.Join(",", FavoriteDestinations));
    }

    public void Load(IPluginStorage storage)
    {
        GoArrowSettings? structured = null;
        try { structured = storage.ReadJson<GoArrowSettings>("settings.json"); }
        catch (Exception) { /* malformed structured data falls back to legacy keys */ }
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
            HudClickThrough = structured.HudClickThrough;
            HudScale = structured.HudScale > 0 ? structured.HudScale : 1;
            MapVisible = structured.MapVisible;
            MapCenterEastWest = structured.MapCenterEastWest;
            MapCenterNorthSouth = structured.MapCenterNorthSouth;
            MapWidth = structured.MapWidth > 0 ? structured.MapWidth : 1000;
            MapHeight = structured.MapHeight > 0 ? structured.MapHeight : 1000;
            ArrivalDistance = structured.ArrivalDistance > 0 ? structured.ArrivalDistance : 0.5;
            UseNavigationAutomation = structured.UseNavigationAutomation;
            NavigationLocked = structured.NavigationLocked;
            RouteCostProfile = structured.RouteCostProfile;
            MaxNavigationRetries = Math.Max(0, structured.MaxNavigationRetries);
            InteractionTimeoutSeconds = structured.InteractionTimeoutSeconds > 0 ? structured.InteractionTimeoutSeconds : 15;
            AtlasCacheMaxAgeDays = Math.Max(0, structured.AtlasCacheMaxAgeDays);
            ExternalDataUrl = string.IsNullOrWhiteSpace(structured.ExternalDataUrl) ? ExternalDataUrl : structured.ExternalDataUrl;
            LastPortalRecall = structured.LastPortalRecall;
            LastSecondaryRecall = structured.LastSecondaryRecall;
            LastAllegianceRecall = structured.LastAllegianceRecall;
            LastHouseRecall = structured.LastHouseRecall;
            LastMansionRecall = structured.LastMansionRecall;
            FavoriteDestinations = structured.FavoriteDestinations ?? new List<string>();
            return;
        }

        DestinationName = storage.ReadText("destination") ?? string.Empty;

        bool.TryParse(storage.ReadText("autoNavigate"), out bool autoNav);
        AutoNavigate = autoNav;

        bool.TryParse(storage.ReadText("recalculate"), out bool recalc);
        RecalculateRoute = recalc;

        bool.TryParse(storage.ReadText("panelVisible"), out bool panelVis);
        PanelVisible = panelVis;

        bool.TryParse(storage.ReadText("showDistance"), out bool showDist);
        ShowDistance = showDist;

        bool.TryParse(storage.ReadText("showBearing"), out bool showBear);
        ShowBearing = showBear;
        bool.TryParse(storage.ReadText("hudVisible"), out bool hudVisible);
        HudVisible = hudVisible;
        bool.TryParse(storage.ReadText("toolbarVisible"), out bool toolbarVisible);
        ToolbarVisible = toolbarVisible;
        bool.TryParse(storage.ReadText("hudClickThrough"), out bool hudClickThrough);
        HudClickThrough = hudClickThrough;
        if (double.TryParse(storage.ReadText("hudScale"), NumberStyles.Float, CultureInfo.InvariantCulture, out double hudScale)
            && hudScale > 0)
            HudScale = hudScale;
        bool.TryParse(storage.ReadText("mapVisible"), out bool mapVisible);
        MapVisible = mapVisible;
        if (double.TryParse(storage.ReadText("mapCenterEW"), NumberStyles.Float, CultureInfo.InvariantCulture, out double mapEW)) MapCenterEastWest = mapEW;
        if (double.TryParse(storage.ReadText("mapCenterNS"), NumberStyles.Float, CultureInfo.InvariantCulture, out double mapNS)) MapCenterNorthSouth = mapNS;
        if (double.TryParse(storage.ReadText("mapWidth"), NumberStyles.Float, CultureInfo.InvariantCulture, out double mapWidth) && mapWidth > 0) MapWidth = mapWidth;
        if (double.TryParse(storage.ReadText("mapHeight"), NumberStyles.Float, CultureInfo.InvariantCulture, out double mapHeight) && mapHeight > 0) MapHeight = mapHeight;

        double.TryParse(storage.ReadText("arrivalDistance"), NumberStyles.Float, CultureInfo.InvariantCulture, out double arrDist);
        ArrivalDistance = arrDist > 0 ? arrDist : 0.5;

        bool.TryParse(storage.ReadText("useNavigation"), out bool useNav);
        UseNavigationAutomation = useNav;
        bool.TryParse(storage.ReadText("navigationLocked"), out bool navigationLocked);
        NavigationLocked = navigationLocked;
        if (Enum.TryParse(storage.ReadText("routeCostProfile"), true, out RouteFinding.RouteCostProfile profile))
            RouteCostProfile = profile;
        if (int.TryParse(storage.ReadText("maxNavigationRetries"), out int retries))
            MaxNavigationRetries = Math.Max(0, retries);
        if (double.TryParse(storage.ReadText("interactionTimeoutSeconds"), NumberStyles.Float, CultureInfo.InvariantCulture, out double timeout)
            && timeout > 0)
            InteractionTimeoutSeconds = timeout;
        if (int.TryParse(storage.ReadText("atlasCacheMaxAgeDays"), out int cacheDays))
            AtlasCacheMaxAgeDays = Math.Max(0, cacheDays);

        ExternalDataUrl = storage.ReadText("externalDataUrl")
            ?? RouteFinding.WarcryAtlasDataProvider.DefaultUrl;
        if (string.IsNullOrWhiteSpace(ExternalDataUrl))
            ExternalDataUrl = RouteFinding.WarcryAtlasDataProvider.DefaultUrl;

        LastPortalRecall = storage.ReadText("lastPortalRecall") ?? string.Empty;
        LastSecondaryRecall = storage.ReadText("lastSecondaryRecall") ?? string.Empty;
        LastAllegianceRecall = storage.ReadText("lastAllegianceRecall") ?? string.Empty;
        LastHouseRecall = storage.ReadText("lastHouseRecall") ?? string.Empty;
        LastMansionRecall = storage.ReadText("lastMansionRecall") ?? string.Empty;

        var favs = storage.ReadText("favorites") ?? string.Empty;
        FavoriteDestinations = favs.Split(new[] { ',' }, StringSplitOptions.RemoveEmptyEntries).ToList();
    }
}