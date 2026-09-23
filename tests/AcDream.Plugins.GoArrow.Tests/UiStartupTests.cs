using System.Xml.Linq;
using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class UiStartupTests
{
    [Fact]
    public void SaveKeepsSettingsInOneJsonFileAndPreservesCachedData()
    {
        var storage = new FakePluginStorage();
        storage.WriteText("showDistance", "True");
        storage.WriteText("destination", "Old destination");
        storage.WriteText("data/warcry-atlas.xml", "<atlas />");
        new GoArrowSettings
        {
            DestinationName = "New destination",
            ShowDistance = false,
            RecallsByCharacter = new() { ["character/world"] = new() { Lifestone = "1,2" } },
        }.Save(storage);

        Assert.Equal(
            new[] { "data/warcry-atlas.xml", "settings.json" },
            storage.Store.Keys.OrderBy(key => key)
        );
        var reloaded = new GoArrowSettings();
        reloaded.Load(storage);
        Assert.Equal("New destination", reloaded.DestinationName);
        Assert.False(reloaded.ShowDistance);
        Assert.Equal("1,2", reloaded.RecallsByCharacter["character/world"].Lifestone);
    }

    [Fact]
    public void LegacyFilesMigrateToJsonAndAreRemovedAfterSuccessfulWrite()
    {
        var storage = new FakePluginStorage();
        storage.WriteText("destination", "Sawato");
        storage.WriteText("autoNavigate", "True");
        storage.WriteText("favorites", "Sawato,Shoushi");
        var settings = new GoArrowSettings();

        settings.Load(storage);

        Assert.Equal("Sawato", settings.DestinationName);
        Assert.True(settings.AutoNavigate);
        Assert.Equal(new[] { "Sawato", "Shoushi" }, settings.FavoriteDestinations);
        Assert.Equal(new[] { "settings.json" }, storage.Store.Keys);
    }

    [Fact]
    public void LegacyRecallFilesMoveIntoCharacterSettings()
    {
        var host = new FakePluginHost
        {
            SettingsValue = new Dictionary<string, string>
            {
                ["characterId"] = "123",
                ["worldId"] = "456",
            },
        };
        host.PluginStorage.WriteText("recall/lifestone", "1,2");
        host.PluginStorage.WriteText("recall/house", "3,4");

        new GoArrowPlugin().Initialize(host);

        GoArrowSettings saved = host.PluginStorage.ReadJson<GoArrowSettings>("settings.json")!;
        Assert.Equal("1,2", saved.RecallsByCharacter["123/456"].Lifestone);
        Assert.Equal("3,4", saved.RecallsByCharacter["123/456"].House);
        Assert.Null(host.PluginStorage.ReadText("recall/lifestone"));
        Assert.Null(host.PluginStorage.ReadText("recall/house"));
    }

    [Fact]
    public void PanelTitleShowsBuiltPluginVersion()
    {
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location)!;
        var markup = XDocument.Load(Path.Combine(directory, "goarrow-panel.xml"));
        XElement title = markup
            .Root!.Elements("label")
            .Single(element => (string?)element.Attribute("text") == "{TitleText}");

        Assert.Equal("8", (string?)title.Attribute("x"));
        Assert.Equal(typeof(string), typeof(GoArrowPanel).GetProperty("TitleText")!.PropertyType);
        Assert.StartsWith("GoArrow v", GoArrowPlugin.DisplayTitle);
        Assert.Contains(
            typeof(GoArrowPlugin).Assembly.GetName().Version!.ToString(3),
            GoArrowPlugin.DisplayTitle
        );
    }

    [Fact]
    public void PositionsSavedByOldDragHandlerReturnToVisibleDefaults()
    {
        var storage = new FakePluginStorage();
        storage.WriteJson(
            "settings.json",
            new GoArrowSettings
            {
                ArrowOffsetX = 5000,
                ToolbarOffsetY = 5000,
                DungeonOffsetX = 5000,
                OverlayPositionVersion = 0,
            }
        );

        var settings = new GoArrowSettings();
        settings.Load(storage);

        Assert.Equal(-70, settings.ArrowOffsetX);
        Assert.Equal(215, settings.ToolbarOffsetY);
        Assert.Equal(25, settings.DungeonOffsetX);
    }

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
        Assert.Equal(string.Empty, settings.DungeonMapUrl);
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
        storage = new FakePluginStorage();
        storage.WriteText("hudVisible", "False");
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
            UseNavigationAutomation = false,
        };
        storage.WriteJson("settings.json", oldSettings);

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
        var oldSettings = new GoArrowSettings
        {
            PanelVisible = true,
            HudVisible = false,
            ToolbarVisible = false,
            MapVisible = false,
            ShowDistance = true,
            ShowBearing = true,
        };
        storage.WriteJson("settings.json", oldSettings);

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
            Assert.Equal(
                typeof(Action<string>),
                typeof(GoArrowPanel)
                    .GetProperty(BindingName(field.Attribute("onsubmit")!.Value))!
                    .PropertyType
            );
            Assert.Equal(
                typeof(Action<string>),
                typeof(GoArrowPanel)
                    .GetProperty(BindingName(field.Attribute("onchange")!.Value))!
                    .PropertyType
            );
        }
        foreach (XElement button in markup.Descendants("button").Take(2))
            Assert.Equal(
                typeof(Action),
                typeof(GoArrowPanel)
                    .GetProperty(BindingName(button.Attribute("onclick")!.Value))!
                    .PropertyType
            );
    }

    [Fact]
    public void RouteAndConfigTabsBindToExclusivePanelViews()
    {
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location)!;
        var markup = XDocument.Load(Path.Combine(directory, "goarrow-panel.xml"));
        XElement[] tabs = markup.Descendants("tab").ToArray();
        Assert.Equal(
            new[] { "Route", "Config", "Details" },
            tabs.Select(tab => (string?)tab.Attribute("text"))
        );
        foreach (XElement tab in tabs)
        {
            Assert.Equal(
                typeof(bool),
                typeof(GoArrowPanel)
                    .GetProperty(BindingName(tab.Attribute("selected")!.Value))!
                    .PropertyType
            );
            Assert.Equal(
                typeof(Action),
                typeof(GoArrowPanel)
                    .GetProperty(BindingName(tab.Attribute("onclick")!.Value))!
                    .PropertyType
            );
        }
        foreach (XElement group in markup.Descendants("group"))
            Assert.Equal(
                typeof(bool),
                typeof(GoArrowPanel)
                    .GetProperty(BindingName(group.Attribute("visible")!.Value))!
                    .PropertyType
            );
    }

    [Fact]
    public void RouteListBindsToPanelStepsAndSelection()
    {
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location)!;
        var markup = XDocument.Load(Path.Combine(directory, "goarrow-panel.xml"));
        XElement list = markup
            .Descendants("list")
            .Single(element => (string?)element.Attribute("items") == "{RouteSteps}");

        Assert.Equal(
            typeof(IReadOnlyList<string>),
            typeof(GoArrowPanel)
                .GetProperty(BindingName(list.Attribute("items")!.Value))!
                .PropertyType
        );
        Assert.Equal(
            typeof(int),
            typeof(GoArrowPanel)
                .GetProperty(BindingName(list.Attribute("selected")!.Value))!
                .PropertyType
        );
        Assert.Equal(
            typeof(Action<int>),
            typeof(GoArrowPanel)
                .GetProperty(BindingName(list.Attribute("onchange")!.Value))!
                .PropertyType
        );
    }

    [Fact]
    public void SearchResultsListBindsToChoicesAndSelectionAction()
    {
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location)!;
        var markup = XDocument.Load(Path.Combine(directory, "goarrow-panel.xml"));
        XElement list = markup
            .Descendants("list")
            .Single(element => (string?)element.Attribute("items") == "{SearchResults}");

        Assert.Equal(
            typeof(IReadOnlyList<string>),
            typeof(GoArrowPanel)
                .GetProperty(BindingName(list.Attribute("items")!.Value))!
                .PropertyType
        );
        Assert.Equal(
            typeof(Action<int>),
            typeof(GoArrowPanel)
                .GetProperty(BindingName(list.Attribute("onchange")!.Value))!
                .PropertyType
        );

        foreach (
            (string input, string editor) in new[]
            {
                ("FromInput", "FromEditorInput"),
                ("DestinationInput", "DestinationInput"),
            }
        )
        {
            XElement field = markup
                .Descendants("field")
                .Single(element => (string?)element.Attribute("text") == $"{{{editor}}}");
            XElement selected = markup
                .Descendants("button")
                .Single(element => (string?)element.Attribute("text") == $"{{{input}}}");
            Assert.Equal(
                typeof(bool),
                typeof(GoArrowPanel)
                    .GetProperty(BindingName(field.Attribute("visible")!.Value))!
                    .PropertyType
            );
            Assert.Equal(
                typeof(bool),
                typeof(GoArrowPanel)
                    .GetProperty(BindingName(selected.Attribute("visible")!.Value))!
                    .PropertyType
            );
            Assert.Equal(
                typeof(Action),
                typeof(GoArrowPanel)
                    .GetProperty(BindingName(selected.Attribute("onclick")!.Value))!
                    .PropertyType
            );
        }
    }

    [Fact]
    public void DungeonMapToggleBindsToVisibleSettingAndAction()
    {
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location)!;
        var markup = XDocument.Load(Path.Combine(directory, "goarrow-panel.xml"));
        XElement toggle = markup
            .Descendants("toggle")
            .Single(element => (string?)element.Attribute("text") == "Dungeon Map");

        Assert.Equal(
            typeof(bool),
            typeof(GoArrowPanel)
                .GetProperty(BindingName(toggle.Attribute("checked")!.Value))!
                .PropertyType
        );
        Assert.Equal(
            typeof(Action),
            typeof(GoArrowPanel)
                .GetProperty(BindingName(toggle.Attribute("onclick")!.Value))!
                .PropertyType
        );
    }

    [Fact]
    public void ConfigOverlayTogglesPersistIndependentVisibility()
    {
        var host = new FakePluginHost();
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        GoArrowPanel panel = plugin.Panel!;
        string directory = Path.GetDirectoryName(typeof(GoArrowPlugin).Assembly.Location)!;
        var markup = XDocument.Load(Path.Combine(directory, "goarrow-panel.xml"));

        foreach (string label in new[] { "Show Arrow", "Show Toolbar" })
        {
            XElement toggle = markup
                .Descendants("toggle")
                .Single(element => (string?)element.Attribute("text") == label);
            Assert.Equal(
                typeof(bool),
                typeof(GoArrowPanel)
                    .GetProperty(BindingName(toggle.Attribute("checked")!.Value))!
                    .PropertyType
            );
            Assert.Equal(
                typeof(Action),
                typeof(GoArrowPanel)
                    .GetProperty(BindingName(toggle.Attribute("onclick")!.Value))!
                    .PropertyType
            );
        }

        panel.ToggleArrowVisible();
        Assert.False(panel.ArrowVisible);
        Assert.True(panel.ToolbarVisible);
        panel.ToggleToolbarVisible();
        Assert.False(panel.ToolbarVisible);

        var reloaded = new GoArrowSettings();
        reloaded.Load(host.PluginStorage);
        Assert.False(reloaded.HudVisible);
        Assert.False(reloaded.ToolbarVisible);
        panel.ToggleArrowVisible();
        Assert.True(panel.ArrowVisible);
        Assert.False(panel.ToolbarVisible);
    }

    private static string BindingName(string value) => value.Trim('{', '}');
}
