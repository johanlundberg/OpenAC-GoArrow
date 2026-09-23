using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class PortalNavigationTests
{
    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void PortalRouteWaitsForTransitionThenContinuesWalking(bool arrivesIndoors)
    {
        var host = new NavigationHost();
        var objects = new TestWorldObjects();
        objects.Objects.Add(new PluginWorldObject(42, 0, "Entrance to Somewhere", default, 0, 0, 0)
        {
            Capabilities = PluginObjectCapabilities.Portal | PluginObjectCapabilities.Interactable,
            HasPosition = true,
            Position = new PluginNavigationPosition(0, 2, 0, 0, 0, true)
        });
        host.Inner.AutomationValue = new FakeAutomationSurface
        {
            Navigation = host.Inner.PluginNavigation,
            Chat = host.Inner.PluginChat,
            Objects = objects
        };
        host.Inner.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(0, 1, 0, 0, 0, true), false, false);

        var db = new LocationDatabase();
        db.LoadLocationsXml("""
            <atlas>
              <location><id>1</id><name>Start</name><type>Town</type><latitude>0</latitude><longitude>1</longitude><retired>N</retired></location>
              <location><id>2</id><name>Far Portal</name><type>Wilderness Portal</type><latitude>0</latitude><longitude>2</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>95</arrival_longitude><retired>N</retired></location>
              <location><id>3</id><name>End</name><type>Town</type><latitude>0</latitude><longitude>96</longitude><retired>N</retired></location>
            </atlas>
            """);
        var settings = new GoArrowSettings { AutoNavigate = true };
        var destination = new GoArrowDestination(settings, db, new RouteFinder(db));
        Assert.True(destination.SetDestination("End"));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        navigator.Enable();
        navigator.StartNavigation();

        Assert.Equal(2, host.Inner.PluginNavigation.GoToPositionCalls[0].Position.EastWest);
        host.EventsValue.RaiseNavigationChanged(new PluginGoToReport(1, PluginGoToState.Arrived, 0, 0, 0, null)
        {
            Revision = 1
        });

        Assert.Equal(new uint[] { 42 }, objects.Activated);
        Assert.True(navigator.WaitingForInteraction);
        Assert.Equal(RouteStepKind.Portal, destination.CurrentRoute!.Steps[0].Kind);
        Assert.Single(host.Inner.PluginNavigation.GoToPositionCalls);

        host.EventsValue.RaiseActivationCompleted(
            new PluginActivationCompletion(1, 42, PluginActivationOutcome.Completed, 0));
        Assert.True(navigator.WaitingForInteraction);
        Assert.Equal(RouteStepKind.Portal, destination.CurrentRoute.Steps[0].Kind);
        Assert.Single(host.Inner.PluginNavigation.GoToPositionCalls);

        host.EventsValue.RaisePortalTransition(new PluginPortalTransition(
            1, 1, 0, true, true, true, false) { Kind = PluginPortalTransitionKind.Login });
        Assert.True(navigator.WaitingForInteraction);
        if (arrivesIndoors)
            host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
            {
                Position = new PluginNavigationPosition(0x12340100, 95, 0, 0, 0, false)
            };
        host.EventsValue.RaisePortalTransition(new PluginPortalTransition(
            2, 2, 0, true, true, true, false) { Kind = PluginPortalTransitionKind.Portal });

        Assert.False(navigator.WaitingForInteraction);
        if (arrivesIndoors)
        {
            Assert.Single(host.Inner.PluginNavigation.GoToPositionCalls);
            host.Inner.PluginNavigation.SnapshotValue = host.Inner.PluginNavigation.SnapshotValue with
            {
                Position = new PluginNavigationPosition(0, 95, 0, 0, 0, true)
            };
            navigator.OnTick(0.1);
        }
        Assert.Equal(96, host.Inner.PluginNavigation.GoToPositionCalls[1].Position.EastWest);
        Assert.Equal(RouteStepKind.Travel, destination.CurrentRoute.Steps[0].Kind);
    }

    private sealed class TestWorldObjects : IWorldObjectAutomation
    {
        public List<PluginWorldObject> Objects { get; } = [];
        public List<uint> Activated { get; } = [];
        public IReadOnlyList<PluginWorldObject> CaptureObjects() => Objects;
        public PluginItemCommandResult Activate(uint objectId)
        {
            Activated.Add(objectId);
            return new PluginItemCommandResult(PluginItemCommandStatus.Started);
        }
    }

    private sealed class NavigationEvents : IEvents
    {
        public event Action<WorldEntitySnapshot>? EntitySpawned { add { } remove { } }
        public event Action<double>? Tick { add { } remove { } }
        public event Action<PluginGoToReport>? NavigationChanged;
        public event Action<PluginActivationCompletion>? ActivationCompleted;
        public event Action<PluginPortalTransition>? PortalTransition;
        public void RaiseNavigationChanged(PluginGoToReport report) => NavigationChanged?.Invoke(report);
        public void RaiseActivationCompleted(PluginActivationCompletion completion) => ActivationCompleted?.Invoke(completion);
        public void RaisePortalTransition(PluginPortalTransition transition) => PortalTransition?.Invoke(transition);
    }

    private sealed class NavigationHost : IPluginHost
    {
        public FakePluginHost Inner { get; } = new();
        public NavigationEvents EventsValue { get; } = new();
        public bool HasUi => Inner.HasUi;
        public IPluginLogger Log => Inner.Log;
        public IGameState State => Inner.State;
        public IEvents Events => EventsValue;
        public ISelectionService Selection => Inner.Selection;
        public IUiRegistry Ui => Inner.Ui;
        public IPluginCommandRegistry Commands => Inner.Commands;
        public IPluginStorage Storage => Inner.Storage;
        public IAutomationSurface Automation => Inner.Automation;
        public IPluginStorage VtankProfiles => Inner.VtankProfiles;
        public IPluginClipboard Clipboard => Inner.Clipboard;
        public IHostWindow Window => Inner.Window;
        public IReadOnlyDictionary<string, string> SessionSettings => Inner.SessionSettings;
    }
}
