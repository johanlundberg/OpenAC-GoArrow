using AcDream.Plugin.Tests.Fixtures;
using AcDream.Plugins.GoArrow;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class LifecycleIntegrationTests
{
    [Fact]
    public void HeadlessPluginCanEnableDisableAndReenable()
    {
        var host = new FakePluginHost { HasUiValue = false };
        var plugin = new GoArrowPlugin();

        plugin.Initialize(host);
        plugin.Enable();
        plugin.Disable();
        plugin.Enable();
        plugin.Disable();

        Assert.Empty(host.PluginLogger.Errors);
    }
}
