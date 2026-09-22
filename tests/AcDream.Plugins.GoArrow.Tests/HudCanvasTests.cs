using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class HudCanvasTests
{
    [Fact]
    public void VisibleArrowUsesCanvasAndTurnsTowardFirstWaypoint()
    {
        var fake = new FakePluginHost();
        fake.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(0, 0, 0, 0, 0, true), false, false);
        var ui = new RecordingUi();
        var host = new CanvasHost(fake, ui);
        var database = new LocationDatabase();
        database.LoadLocationsCsv(new[] { "Start;0;0", "First;0;10", "Finish;10;10" });
        var settings = new GoArrowSettings();
        var destination = new GoArrowDestination(settings, database, new RouteFinder(database));
        destination.SetDestination("Finish");
        destination.CalculateRoute(new Location("Current", 0, 0));
        using var hud = new GoArrowHud(host, destination, new GoArrowNavigator(host, destination, settings), settings);

        hud.Enable();

        Assert.Contains("goarrow.arrow", ui.PaintCallbacks.Keys);
        Assert.Contains("goarrow.toolbar", ui.PaintCallbacks.Keys);
        var painter = new RecordingPainter();
        ui.PaintCallbacks["goarrow.arrow"](painter);
        var shaft = painter.Lines.First(line => line.Thickness == 12);
        Assert.True(shaft.To.X > shaft.From.X);
        Assert.Equal(shaft.From.Y, shaft.To.Y, 6);

        fake.PluginNavigation.SnapshotValue = fake.PluginNavigation.SnapshotValue with
        {
            Position = new PluginNavigationPosition(0, 0, 0, 0, 90, true)
        };
        painter.Lines.Clear();
        ui.PaintCallbacks["goarrow.arrow"](painter);
        shaft = painter.Lines.First(line => line.Thickness == 12);
        Assert.True(shaft.To.Y < shaft.From.Y);
    }

    private sealed class CanvasHost(FakePluginHost inner, IUiRegistry ui) : IPluginHost
    {
        public bool HasUi => true;
        public IPluginLogger Log => inner.Log;
        public IGameState State => inner.State;
        public IEvents Events => inner.Events;
        public ISelectionService Selection => inner.Selection;
        public IUiRegistry Ui => ui;
        public IAutomationSurface Automation => inner.Automation;
    }

    private sealed class RecordingUi : IUiRegistry
    {
        public Dictionary<string, Action<IPluginPainter>> PaintCallbacks { get; } = new();

        public void AddMarkupPanel(string markupPath, object binding) { }

        public IPluginCanvas RegisterCanvas(PluginCanvasDescriptor descriptor, Action<IPluginPainter> paint)
        {
            PaintCallbacks.Add(descriptor.CanvasId, paint);
            return new NoOpPluginCanvas(descriptor);
        }
    }

    private sealed class RecordingPainter : IPluginPainter
    {
        public int Width => 260;
        public int Height => 90;
        public List<(PluginPoint From, PluginPoint To, float Thickness)> Lines { get; } = new();
        public void Clear(PluginColor color) { }
        public void FillRect(PluginRect rect, PluginColor color) { }
        public void StrokeRect(PluginRect rect, PluginColor color, float thickness = 1f) { }
        public void DrawLine(PluginPoint from, PluginPoint to, PluginColor color, float thickness = 1f) =>
            Lines.Add((from, to, thickness));
        public void DrawText(string text, PluginPoint position, PluginColor color, bool outline = false) { }
        public PluginSize MeasureText(string text) => new(0, 0);
        public void DrawImage(PluginImage image, PluginRect destination, PluginColor tint) { }
        public void DrawImageTransformed(
            PluginImage image, PluginRect destination, PluginColor tint, double rotationRadians,
            PluginPoint pivot, double scaleX = 1.0, double scaleY = 1.0) { }
        public void PushClip(PluginRect rect) { }
        public void PopClip() { }
    }
}
