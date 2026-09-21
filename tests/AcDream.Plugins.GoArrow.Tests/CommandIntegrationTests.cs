using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;
using AcDream.Plugins.GoArrow;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class CommandIntegrationTests
{
    [Fact]
    public void CoordinateCommandAndChatLinkSetCoordinateDestination()
    {
        var host = new FakePluginHost { HasUiValue = false };
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();

        Assert.True(host.PluginCommands.Invoke("go", "to 42.1N 33.6E"));
        Assert.Equal("42.1N 33.6E", plugin.CurrentDestinationName);

        host.PluginChat.RaiseLinkClicked(new PluginChatLinkClicked(
            PluginChatLinkKind.Coordinate, "7N 8E", new PluginChatCoordinate(8, 7)));
        Assert.Equal("7N 8E", plugin.CurrentDestinationName);

        plugin.Disable();
    }
}
