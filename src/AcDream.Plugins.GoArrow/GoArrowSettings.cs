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

    // ── Navigation ─────────────────────────────────────────────────
    public double ArrivalDistance { get; set; } = 0.5;
    public bool UseNavigationAutomation { get; set; } = true;

    // ── External data ──────────────────────────────────────────────
    /// <summary>Location-data URL used by the explicit update command.</summary>
    public string ExternalDataUrl { get; set; } =
        RouteFinding.WarcryAtlasDataProvider.DefaultUrl;

    // ── Recall Tracking ────────────────────────────────────────────
    public string LastPortalRecall { get; set; } = string.Empty;
    public string LastSecondaryRecall { get; set; } = string.Empty;
    public string LastAllegianceRecall { get; set; } = string.Empty;

    // ── Favorites ──────────────────────────────────────────────────
    public List<string> FavoriteDestinations { get; set; } = new();

    public void Save(IPluginStorage storage)
    {
        storage.WriteText("destination", DestinationName);
        storage.WriteText("autoNavigate", AutoNavigate.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("recalculate", RecalculateRoute.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("panelVisible", PanelVisible.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("showDistance", ShowDistance.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("showBearing", ShowBearing.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("arrivalDistance", ArrivalDistance.ToString("F4", CultureInfo.InvariantCulture));
        storage.WriteText("useNavigation", UseNavigationAutomation.ToString(CultureInfo.InvariantCulture));
        storage.WriteText("externalDataUrl", ExternalDataUrl);
        storage.WriteText("lastPortalRecall", LastPortalRecall);
        storage.WriteText("lastSecondaryRecall", LastSecondaryRecall);
        storage.WriteText("lastAllegianceRecall", LastAllegianceRecall);
        storage.WriteText("favorites", string.Join(",", FavoriteDestinations));
    }

    public void Load(IPluginStorage storage)
    {
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

        double.TryParse(storage.ReadText("arrivalDistance"), NumberStyles.Float, CultureInfo.InvariantCulture, out double arrDist);
        ArrivalDistance = arrDist > 0 ? arrDist : 0.5;

        bool.TryParse(storage.ReadText("useNavigation"), out bool useNav);
        UseNavigationAutomation = useNav;

        ExternalDataUrl = storage.ReadText("externalDataUrl")
            ?? RouteFinding.WarcryAtlasDataProvider.DefaultUrl;
        if (string.IsNullOrWhiteSpace(ExternalDataUrl))
            ExternalDataUrl = RouteFinding.WarcryAtlasDataProvider.DefaultUrl;

        LastPortalRecall = storage.ReadText("lastPortalRecall") ?? string.Empty;
        LastSecondaryRecall = storage.ReadText("lastSecondaryRecall") ?? string.Empty;
        LastAllegianceRecall = storage.ReadText("lastAllegianceRecall") ?? string.Empty;

        var favs = storage.ReadText("favorites") ?? string.Empty;
        FavoriteDestinations = favs.Split(new[] { ',' }, StringSplitOptions.RemoveEmptyEntries).ToList();
    }
}