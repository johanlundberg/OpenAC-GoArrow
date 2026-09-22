using System.Xml.Linq;
using AcDream.Plugin.Tests.Fixtures;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class UiStartupTests
{
    [Fact]
    public void NewInstallKeepsVisibleUiDefaults()
    {
        var settings = new GoArrowSettings();

        settings.Load(new FakePluginStorage());

        Assert.True(settings.PanelVisible);
        Assert.True(settings.HudVisible);
        Assert.True(settings.ToolbarVisible);
        Assert.True(settings.MapVisible);
        Assert.True(settings.ShowDistance);
        Assert.True(settings.ShowBearing);
        Assert.True(settings.RecalculateRoute);
        Assert.True(settings.UseNavigationAutomation);
        Assert.Equal(WarcryAtlasDataProvider.DefaultUrl, settings.ExternalDataUrl);
    }

    [Fact]
    public void SavedPreviousDefaultAtlasUrlMovesToPortalAtlas()
    {
        var storage = new FakePluginStorage();
        storage.WriteText("externalDataUrl", WarcryAtlasDataProvider.PreviousDefaultUrl);
        var settings = new GoArrowSettings();

        settings.Load(storage);

        Assert.Equal(WarcryAtlasDataProvider.DefaultUrl, settings.ExternalDataUrl);
    }

    [Fact]
    public void LegacySettingsPreserveExplicitFalseAndRecoverAnAllHiddenProfile()
    {
        var storage = new FakePluginStorage();
        storage.WriteText("hudVisible", "False");
        var settings = new GoArrowSettings();

        settings.Load(storage);

        Assert.False(settings.HudVisible);
        Assert.True(settings.PanelVisible);
        storage.WriteText("panelVisible", "False");
        storage.WriteText("toolbarVisible", "False");
        storage.WriteText("mapVisible", "False");
        settings.Load(storage);
        Assert.True(settings.PanelVisible);
        Assert.True(settings.HudVisible);
    }

    [Fact]
    public void PreviouslySavedAllHiddenProfileRecoversThePanel()
    {
        var storage = new FakePluginStorage();
        var oldSettings = new GoArrowSettings
        {
            PanelVisible = false,
            HudVisible = false,
            ToolbarVisible = false,
            MapVisible = false,
            ShowDistance = false,
            ShowBearing = false,
            RecalculateRoute = false,
            UseNavigationAutomation = false
        };
        oldSettings.Save(storage);

        var settings = new GoArrowSettings();
        settings.Load(storage);

        Assert.True(settings.PanelVisible);
        Assert.True(settings.HudVisible);
        Assert.False(settings.ToolbarVisible);
        Assert.True(settings.ShowDistance);
        Assert.True(settings.ShowBearing);
        Assert.True(settings.RecalculateRoute);
        Assert.True(settings.UseNavigationAutomation);
    }

    [Fact]
    public void PanelOnlyRepairAlsoRestoresPreviouslyHiddenHud()
    {
        var storage = new FakePluginStorage();
        new GoArrowSettings
        {
            PanelVisible = true,
            HudVisible = false,
            ToolbarVisible = false,
            MapVisible = false,
            ShowDistance = true,
            ShowBearing = true
        }.Save(storage);

        var settings = new GoArrowSettings();
        settings.Load(storage);

        Assert.True(settings.HudVisible);
        Assert.False(settings.ToolbarVisible);
    }

    [Fact]
    public void DestinationFieldUsesTheCallbackTypeRequiredByOpenAc()
    {
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location)!;
        var markup = XDocument.Load(Path.Combine(directory, "goarrow-panel.xml"));
        XElement field = markup.Descendants("field").Single();
        string fieldAction = BindingName(field.Attribute("onsubmit")!.Value);
        string changeAction = BindingName(field.Attribute("onchange")!.Value);
        string buttonAction = BindingName(markup.Descendants("button").First().Attribute("onclick")!.Value);

        Assert.Equal(typeof(Action<string>), typeof(GoArrowPanel).GetProperty(fieldAction)!.PropertyType);
        Assert.Equal(typeof(Action<string>), typeof(GoArrowPanel).GetProperty(changeAction)!.PropertyType);
        Assert.Equal(typeof(Action), typeof(GoArrowPanel).GetProperty(buttonAction)!.PropertyType);
    }

    [Fact]
    public void RouteListBindsToPanelStepsAndSelection()
    {
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location)!;
        var markup = XDocument.Load(Path.Combine(directory, "goarrow-panel.xml"));
        XElement list = markup.Descendants("list").Single();

        Assert.Equal(typeof(IReadOnlyList<string>),
            typeof(GoArrowPanel).GetProperty(BindingName(list.Attribute("items")!.Value))!.PropertyType);
        Assert.Equal(typeof(int),
            typeof(GoArrowPanel).GetProperty(BindingName(list.Attribute("selected")!.Value))!.PropertyType);
    }

    private static string BindingName(string value) => value.Trim('{', '}');
}
