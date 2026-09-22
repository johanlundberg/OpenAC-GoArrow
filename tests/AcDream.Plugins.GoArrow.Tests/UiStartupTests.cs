using System.Xml.Linq;
using AcDream.Plugin.Tests.Fixtures;

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
            MapVisible = false
        };
        oldSettings.Save(storage);

        var settings = new GoArrowSettings();
        settings.Load(storage);

        Assert.True(settings.PanelVisible);
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

    private static string BindingName(string value) => value.Trim('{', '}');
}
