using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;
using AcDream.Plugins.GoArrow;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class CommandIntegrationTests
{
    [Fact]
    public void StatusShowsLoadedVersionAndCurrentNavigationState()
    {
        var host = new FakePluginHost { HasUiValue = false };
        host.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true,
            false,
            1,
            new PluginNavigationPosition(0, 1, 1, 0, 0, true),
            false,
            false
        );
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();

        Assert.True(host.PluginCommands.Invoke("go", "status"));
        Assert.Contains(
            host.PluginChat.SystemMessages,
            message =>
                message.Contains(GoArrowPlugin.DisplayTitle)
                && message.Contains("outdoors")
                && message.Contains("no active step")
        );
        plugin.Disable();
    }

    [Fact]
    public void MarkCommandAddsAnIndoorLocationToSearchableData()
    {
        var host = new FakePluginHost { HasUiValue = false };
        host.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true,
            false,
            1,
            new PluginNavigationPosition(0x12340122, 10, 10, 0, 0, false),
            false,
            false
        );
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();

        Assert.True(host.PluginCommands.Invoke("go", "mark Lower Chamber"));
        Assert.Equal("Lower Chamber", plugin.CurrentDestinationName);
        Assert.Contains(
            plugin.SearchLocations("Lower"),
            location => location.IndoorPosition?.CellId == 0x12340122
        );
        plugin.Disable();
    }

    [Fact]
    public void DungeonCommandControlsAutomaticMapVisibility()
    {
        var host = new FakePluginHost { HasUiValue = false };
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();

        Assert.True(host.PluginCommands.Invoke("go", "dungeon off"));
        Assert.False(plugin.DungeonMapVisible);
        Assert.False(
            host.PluginStorage.ReadJson<GoArrowSettings>("settings.json")!.DungeonMapVisible
        );
        Assert.True(host.PluginCommands.Invoke("go", "dungeon on"));
        Assert.True(plugin.DungeonMapVisible);

        plugin.Disable();
    }

    [Fact]
    public void CoordinateCommandAndChatLinkSetCoordinateDestination()
    {
        var host = new FakePluginHost { HasUiValue = false };
        var plugin = new GoArrowPlugin();
        plugin.Initialize(host);
        plugin.Enable();

        Assert.True(host.PluginCommands.Invoke("go", "to 42.1N 33.6E"));
        Assert.Equal("42.1N 33.6E", plugin.CurrentDestinationName);

        host.PluginChat.RaiseLinkClicked(
            new PluginChatLinkClicked(
                PluginChatLinkKind.Coordinate,
                "7N 8E",
                new PluginChatCoordinate(8, 7)
            )
        );
        Assert.Equal("7N 8E", plugin.CurrentDestinationName);

        plugin.Disable();
    }
}
