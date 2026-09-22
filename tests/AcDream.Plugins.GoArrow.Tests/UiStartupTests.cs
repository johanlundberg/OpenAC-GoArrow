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
        Assert.True(settings.DungeonMapVisible);
        Assert.True(settings.ShowDistance);
        Assert.True(settings.ShowBearing);
        Assert.True(settings.RecalculateRoute);
        Assert.True(settings.UseNavigationAutomation);
        Assert.Equal(string.Empty, settings.ExternalDataUrl);
        Assert.Equal(DungeonMapDownloader.DefaultUrl, settings.DungeonMapUrl);
    }

    [Fact]
    public void SavedLocationDataUrlIsPreserved()
    {
        var storage = new FakePluginStorage();
        storage.WriteText("externalDataUrl", "https://example.test/locations.xml");
        var settings = new GoArrowSettings();

        settings.Load(storage);

        Assert.Equal("https://example.test/locations.xml", settings.ExternalDataUrl);
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
    public void SearchFieldsUseTheCallbackTypesRequiredByOpenAc()
    {
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location)!;
        var markup = XDocument.Load(Path.Combine(directory, "goarrow-panel.xml"));
        Assert.Equal(4, markup.Descendants("field").Count());
        foreach (XElement field in markup.Descendants("field"))
        {
            Assert.Equal(typeof(Action<string>), typeof(GoArrowPanel)
                .GetProperty(BindingName(field.Attribute("onsubmit")!.Value))!.PropertyType);
            Assert.Equal(typeof(Action<string>), typeof(GoArrowPanel)
                .GetProperty(BindingName(field.Attribute("onchange")!.Value))!.PropertyType);
        }
        foreach (XElement button in markup.Descendants("button").Take(2))
            Assert.Equal(typeof(Action), typeof(GoArrowPanel)
                .GetProperty(BindingName(button.Attribute("onclick")!.Value))!.PropertyType);
    }

    [Fact]
    public void RouteAndConfigTabsBindToExclusivePanelViews()
    {
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location)!;
        var markup = XDocument.Load(Path.Combine(directory, "goarrow-panel.xml"));
        XElement[] tabs = markup.Descendants("tab").ToArray();
        Assert.Equal(new[] { "Route", "Config" },
            tabs.Select(tab => (string?)tab.Attribute("text")));
        foreach (XElement tab in tabs)
        {
            Assert.Equal(typeof(bool), typeof(GoArrowPanel)
                .GetProperty(BindingName(tab.Attribute("selected")!.Value))!.PropertyType);
            Assert.Equal(typeof(Action), typeof(GoArrowPanel)
                .GetProperty(BindingName(tab.Attribute("onclick")!.Value))!.PropertyType);
        }
        foreach (XElement group in markup.Descendants("group"))
            Assert.Equal(typeof(bool), typeof(GoArrowPanel)
                .GetProperty(BindingName(group.Attribute("visible")!.Value))!.PropertyType);
    }

    [Fact]
    public void RouteListBindsToPanelStepsAndSelection()
    {
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location)!;
        var markup = XDocument.Load(Path.Combine(directory, "goarrow-panel.xml"));
        XElement list = markup.Descendants("list")
            .Single(element => (string?)element.Attribute("items") == "{RouteSteps}");

        Assert.Equal(typeof(IReadOnlyList<string>),
            typeof(GoArrowPanel).GetProperty(BindingName(list.Attribute("items")!.Value))!.PropertyType);
        Assert.Equal(typeof(int),
            typeof(GoArrowPanel).GetProperty(BindingName(list.Attribute("selected")!.Value))!.PropertyType);
    }

    [Fact]
    public void SearchResultsListBindsToChoicesAndSelectionAction()
    {
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location)!;
        var markup = XDocument.Load(Path.Combine(directory, "goarrow-panel.xml"));
        XElement list = markup.Descendants("list")
            .Single(element => (string?)element.Attribute("items") == "{SearchResults}");

        Assert.Equal(typeof(IReadOnlyList<string>),
            typeof(GoArrowPanel).GetProperty(BindingName(list.Attribute("items")!.Value))!.PropertyType);
        Assert.Equal(typeof(Action<int>),
            typeof(GoArrowPanel).GetProperty(BindingName(list.Attribute("onchange")!.Value))!.PropertyType);

        foreach ((string input, string editor) in new[]
                 {
                     ("FromInput", "FromEditorInput"),
                     ("DestinationInput", "DestinationInput")
                 })
        {
            XElement field = markup.Descendants("field")
                .Single(element => (string?)element.Attribute("text") == $"{{{editor}}}");
            XElement selected = markup.Descendants("button")
                .Single(element => (string?)element.Attribute("text") == $"{{{input}}}");
            Assert.Equal(typeof(bool), typeof(GoArrowPanel)
                .GetProperty(BindingName(field.Attribute("visible")!.Value))!.PropertyType);
            Assert.Equal(typeof(bool), typeof(GoArrowPanel)
                .GetProperty(BindingName(selected.Attribute("visible")!.Value))!.PropertyType);
            Assert.Equal(typeof(Action), typeof(GoArrowPanel)
                .GetProperty(BindingName(selected.Attribute("onclick")!.Value))!.PropertyType);
        }
    }

    [Fact]
    public void DungeonMapToggleBindsToVisibleSettingAndAction()
    {
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location)!;
        var markup = XDocument.Load(Path.Combine(directory, "goarrow-panel.xml"));
        XElement toggle = markup.Descendants("toggle")
            .Single(element => (string?)element.Attribute("text") == "Dungeon Map");

        Assert.Equal(typeof(bool),
            typeof(GoArrowPanel).GetProperty(BindingName(toggle.Attribute("checked")!.Value))!.PropertyType);
        Assert.Equal(typeof(Action),
            typeof(GoArrowPanel).GetProperty(BindingName(toggle.Attribute("onclick")!.Value))!.PropertyType);
    }

    private static string BindingName(string value) => value.Trim('{', '}');
}
