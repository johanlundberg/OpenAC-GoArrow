using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class HudCanvasTests
{
    [Theory]
    [InlineData(true, true, true, true)]
    [InlineData(true, false, true, false)]
    [InlineData(false, true, false, true)]
    [InlineData(false, false, false, false)]
    public void DisplayTogglesControlArrowReadout(
        bool showBearing, bool showDistance, bool expectBearing, bool expectDistance)
    {
        var fake = new FakePluginHost();
        fake.PluginNavigation.SnapshotValue = new PluginNavigationSnapshot(
            true, false, 1, new PluginNavigationPosition(0, 0, 0, 0, 0, true), false, false);
        var ui = new RecordingUi();
        var host = new CanvasHost(fake, ui);
        var database = new LocationDatabase();
        database.LoadLocationsCsv(new[] { "Start;0;0", "Finish;0;10" });
        var settings = new GoArrowSettings
        {
            ShowBearing = showBearing,
            ShowDistance = showDistance,
        };
        var destination = new GoArrowDestination(settings, database, new RouteFinder(database));
        destination.SetDestination("Finish");
        destination.CalculateRoute(new Location("Current", 0, 0));
        using var hud = new GoArrowHud(host, destination,
            new GoArrowNavigator(host, destination, settings), settings);
        hud.Enable();

        var painter = new RecordingPainter();
        ui.PaintCallbacks["goarrow.arrow"](painter);
        string? readout = painter.Texts.SingleOrDefault(text =>
            text.Contains('°') || text.EndsWith(" m") || text.EndsWith(" km"));
        Assert.Equal(expectBearing, readout?.Contains('°') == true);
        Assert.Equal(expectDistance,
            readout?.EndsWith(" m") == true || readout?.EndsWith(" km") == true);
    }

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
        using var navigator = new GoArrowNavigator(host, destination, settings);
        using var hud = new GoArrowHud(host, destination, navigator, settings);

        hud.Enable();

        Assert.Contains("goarrow.arrow", ui.PaintCallbacks.Keys);
        Assert.Contains("goarrow.toolbar", ui.PaintCallbacks.Keys);
        var painter = new RecordingPainter();
        ui.PaintCallbacks["goarrow.arrow"](painter);
        Assert.DoesNotContain("DRAG", painter.Texts);
        painter.Texts.Clear();
        ui.PaintCallbacks["goarrow.toolbar"](painter);
        Assert.Equal(new[] { "Stop", "Resume" }, painter.Texts);
        var shaft = painter.Lines.First(line => line.Thickness == 9);
        Assert.True(shaft.To.X > shaft.From.X);
        Assert.Equal(shaft.From.Y, shaft.To.Y, 6);

        fake.PluginNavigation.SnapshotValue = fake.PluginNavigation.SnapshotValue with
        {
            Position = new PluginNavigationPosition(0, 0, 0, 0, 90, true)
        };
        painter.Lines.Clear();
        ui.PaintCallbacks["goarrow.arrow"](painter);
        shaft = painter.Lines.First(line => line.Thickness == 9);
        Assert.True(shaft.To.Y < shaft.From.Y);

        var arrow = ui.Canvases["goarrow.arrow"];
        arrow.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Down,
            new PluginPoint(40, 40), PluginPointerButton.Left, PluginKeyModifiers.None));
        arrow.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Move,
            new PluginPoint(70, 60), PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.Equal(new PluginPoint(-40, 140), arrow.Offset);
        arrow.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Move,
            new PluginPoint(70, 60), PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.Equal(new PluginPoint(-40, 140), arrow.Offset);
        fake.PluginEvents.RaiseTick(0.1);
        arrow.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Move,
            new PluginPoint(40, 40), PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.Equal(new PluginPoint(-40, 140), arrow.Offset);
        arrow.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Up,
            new PluginPoint(40, 40), PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.Equal(new PluginPoint(-40, 140), arrow.Offset);
        Assert.Equal(-40, settings.ArrowOffsetX);
        Assert.Equal(-40, fake.PluginStorage.ReadJson<GoArrowSettings>("settings.json")!.ArrowOffsetX);

        var toolbar = ui.Canvases["goarrow.toolbar"];
        toolbar.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Down,
            new PluginPoint(200, 15), PluginPointerButton.Left, PluginKeyModifiers.None));
        toolbar.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Move,
            new PluginPoint(220, 25), PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.Equal(new PluginPoint(-50, 225), toolbar.Offset);
        toolbar.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Move,
            new PluginPoint(220, 25), PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.Equal(new PluginPoint(-50, 225), toolbar.Offset);
        toolbar.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Up,
            new PluginPoint(220, 25), PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.Equal(new PluginPoint(-50, 225), toolbar.Offset);
        Assert.Equal(-50, settings.ToolbarOffsetX);
        Assert.Equal(-50, fake.PluginStorage.ReadJson<GoArrowSettings>("settings.json")!.ToolbarOffsetX);
        fake.PluginEvents.RaiseTick(0.1);
        var saved = new GoArrowSettings();
        saved.Load(fake.Storage);
        Assert.Equal(-40, saved.ArrowOffsetX);
        Assert.Equal(225, saved.ToolbarOffsetY);

        toolbar.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Down,
            new PluginPoint(40, 15), PluginPointerButton.Left, PluginKeyModifiers.None));
        toolbar.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Move,
            new PluginPoint(55, 20), PluginPointerButton.Left, PluginKeyModifiers.None));
        toolbar.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Up,
            new PluginPoint(55, 20), PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.Equal(new PluginPoint(-35, 230), toolbar.Offset);
        Assert.Equal(-35, settings.ToolbarOffsetX);
        Assert.Equal(-35, fake.PluginStorage.ReadJson<GoArrowSettings>("settings.json")!.ToolbarOffsetX);

        navigator.StartNavigation();
        Assert.True(navigator.IsNavigating);
        toolbar.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Down,
            new PluginPoint(40, 15), PluginPointerButton.Left, PluginKeyModifiers.None));
        toolbar.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Up,
            new PluginPoint(40, 15), PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.False(navigator.IsNavigating);
        toolbar.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Down,
            new PluginPoint(190, 15), PluginPointerButton.Left, PluginKeyModifiers.None));
        toolbar.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Up,
            new PluginPoint(190, 15), PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.Equal(new PluginPoint(-35, 230), toolbar.Offset);
    }

    [Fact]
    public void ReleasedOverlayPositionsRestoreOnNewCanvasesWithoutAnotherTick()
    {
        var fake = new FakePluginHost();
        var ui = new RecordingUi();
        var host = new CanvasHost(fake, ui);
        var database = new LocationDatabase();
        var settings = new GoArrowSettings();
        var destination = new GoArrowDestination(settings, database, new RouteFinder(database));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        using var hud = new GoArrowHud(host, destination, navigator, settings);
        hud.Enable();
        var arrow = ui.Canvases["goarrow.arrow"];
        arrow.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Down,
            new PluginPoint(20, 20), PluginPointerButton.Left, PluginKeyModifiers.None));
        arrow.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Up,
            new PluginPoint(50, 45), PluginPointerButton.Left, PluginKeyModifiers.None));
        var toolbar = ui.Canvases["goarrow.toolbar"];
        toolbar.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Down,
            new PluginPoint(20, 20), PluginPointerButton.Left, PluginKeyModifiers.None));
        toolbar.PointerHandler!(new PluginPointerEvent(PluginPointerEventKind.Up,
            new PluginPoint(60, 50), PluginPointerButton.Left, PluginKeyModifiers.None));

        var restoredSettings = new GoArrowSettings();
        restoredSettings.Load(fake.PluginStorage);
        var restoredUi = new RecordingUi();
        var restoredHost = new CanvasHost(fake, restoredUi);
        var restoredDestination = new GoArrowDestination(restoredSettings, database, new RouteFinder(database));
        using var restoredNavigator = new GoArrowNavigator(restoredHost, restoredDestination, restoredSettings);
        using var restoredHud = new GoArrowHud(restoredHost, restoredDestination, restoredNavigator, restoredSettings);
        restoredHud.Enable();

        Assert.Equal(new PluginPoint(-40, 145), restoredUi.Canvases["goarrow.arrow"].Offset);
        Assert.Equal(new PluginPoint(-30, 245), restoredUi.Canvases["goarrow.toolbar"].Offset);
    }

    [Fact]
    public void OverlayVisibilityChangesApplyToActiveCanvases()
    {
        var fake = new FakePluginHost();
        var ui = new RecordingUi();
        var host = new CanvasHost(fake, ui);
        var database = new LocationDatabase();
        var settings = new GoArrowSettings();
        var destination = new GoArrowDestination(settings, database, new RouteFinder(database));
        using var navigator = new GoArrowNavigator(host, destination, settings);
        using var hud = new GoArrowHud(host, destination, navigator, settings);
        hud.Enable();

        hud.SetArrowVisible(false);
        Assert.False(ui.Canvases["goarrow.arrow"].IsVisible);
        Assert.True(ui.Canvases["goarrow.toolbar"].IsVisible);
        hud.SetToolbarVisible(false);
        Assert.False(ui.Canvases["goarrow.toolbar"].IsVisible);
        hud.SetArrowVisible(true);
        Assert.True(ui.Canvases["goarrow.arrow"].IsVisible);
        Assert.False(ui.Canvases["goarrow.toolbar"].IsVisible);
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
        public IPluginStorage Storage => inner.Storage;
    }

    private sealed class RecordingUi : IUiRegistry
    {
        public Dictionary<string, Action<IPluginPainter>> PaintCallbacks { get; } = new();
        public Dictionary<string, NoOpPluginCanvas> Canvases { get; } = new();

        public void AddMarkupPanel(string markupPath, object binding) { }

        public IPluginCanvas RegisterCanvas(PluginCanvasDescriptor descriptor, Action<IPluginPainter> paint)
        {
            PaintCallbacks.Add(descriptor.CanvasId, paint);
            var canvas = new NoOpPluginCanvas(descriptor);
            Canvases.Add(descriptor.CanvasId, canvas);
            return canvas;
        }
    }

    private sealed class RecordingPainter : IPluginPainter
    {
        public int Width => 260;
        public int Height => 90;
        public List<(PluginPoint From, PluginPoint To, float Thickness)> Lines { get; } = new();
        public List<string> Texts { get; } = new();
        public void Clear(PluginColor color) { }
        public void FillRect(PluginRect rect, PluginColor color) { }
        public void StrokeRect(PluginRect rect, PluginColor color, float thickness = 1f) { }
        public void DrawLine(PluginPoint from, PluginPoint to, PluginColor color, float thickness = 1f) =>
            Lines.Add((from, to, thickness));
        public void DrawText(string text, PluginPoint position, PluginColor color, bool outline = false) =>
            Texts.Add(text);
        public PluginSize MeasureText(string text) => new(0, 0);
        public void DrawImage(PluginImage image, PluginRect destination, PluginColor tint) { }
        public void DrawImageTransformed(
            PluginImage image, PluginRect destination, PluginColor tint, double rotationRadians,
            PluginPoint pivot, double scaleX = 1.0, double scaleY = 1.0) { }
        public void PushClip(PluginRect rect) { }
        public void PopClip() { }
    }
}
