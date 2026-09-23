using AcDream.Plugin.Abstractions;
using AcDream.Plugin.Tests.Fixtures;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class DungeonMapCanvasTests
{
    [Fact]
    public void DungeonCanvasFollowsIndoorCellAndHidesOutdoors()
    {
        var fake = new FakePluginHost();
        fake.PluginNavigation.SnapshotValue = Snapshot(0x00010001, isOutdoor: false);
        var ui = new RecordingUi();
        var host = new CanvasHost(fake, ui);
        var catalog = DungeonMapTestData.CreateCatalog();
        var settings = new GoArrowSettings();
        using var map = new GoArrowDungeonMap(host, settings, catalog);

        map.Enable();

        Assert.Equal(0x0001, map.CurrentDungeonId);
        Assert.Equal("Remote Empyrean Vault", map.CurrentMapName);
        Assert.True(ui.Canvas!.IsVisible);
        Assert.Single(ui.Images.Loaded);
        Assert.Equal("dungeon/0001", ui.Images.Loaded[0]);
        var painter = new RecordingPainter();
        ui.Paint!(painter);
        Assert.Single(painter.Images);
        Assert.Contains(painter.Text, text => text.Contains("Remote Empyrean Vault"));
        PluginRect initialImage = Assert.Single(painter.ImageRects);
        ui.Canvas.PointerHandler!(new PluginPointerEvent(
            PluginPointerEventKind.Wheel, new PluginPoint(200, 200),
            PluginPointerButton.None, PluginKeyModifiers.None, 1));
        painter.ImageRects.Clear();
        ui.Paint(painter);
        PluginRect zoomedImage = Assert.Single(painter.ImageRects);
        Assert.True(zoomedImage.Width > initialImage.Width);
        ui.Canvas.PointerHandler!(new PluginPointerEvent(
            PluginPointerEventKind.Down, new PluginPoint(200, 200),
            PluginPointerButton.Left, PluginKeyModifiers.None));
        ui.Canvas.PointerHandler!(new PluginPointerEvent(
            PluginPointerEventKind.Move, new PluginPoint(220, 210),
            PluginPointerButton.Left, PluginKeyModifiers.None));
        ui.Canvas.PointerHandler!(new PluginPointerEvent(
            PluginPointerEventKind.Up, new PluginPoint(220, 210),
            PluginPointerButton.Left, PluginKeyModifiers.None));
        painter.ImageRects.Clear();
        ui.Paint(painter);
        Assert.True(Assert.Single(painter.ImageRects).X > zoomedImage.X);

        ui.Canvas.PointerHandler!(new PluginPointerEvent(
            PluginPointerEventKind.Down, new PluginPoint(100, 15),
            PluginPointerButton.Left, PluginKeyModifiers.None));
        ui.Canvas.PointerHandler!(new PluginPointerEvent(
            PluginPointerEventKind.Move, new PluginPoint(135, 30),
            PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.Equal(new PluginPoint(60, 110), ui.Canvas.Offset);
        ui.Canvas.PointerHandler!(new PluginPointerEvent(
            PluginPointerEventKind.Move, new PluginPoint(135, 30),
            PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.Equal(new PluginPoint(60, 110), ui.Canvas.Offset);
        fake.PluginEvents.RaiseTick(0.1);
        ui.Canvas.PointerHandler!(new PluginPointerEvent(
            PluginPointerEventKind.Move, new PluginPoint(100, 15),
            PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.Equal(new PluginPoint(60, 110), ui.Canvas.Offset);
        ui.Canvas.PointerHandler!(new PluginPointerEvent(
            PluginPointerEventKind.Up, new PluginPoint(100, 15),
            PluginPointerButton.Left, PluginKeyModifiers.None));
        Assert.Equal(new PluginPoint(60, 110), ui.Canvas.Offset);
        Assert.Equal(60, settings.DungeonOffsetX);

        fake.PluginNavigation.SnapshotValue = Snapshot(0x00020001, isOutdoor: false);
        fake.PluginEvents.RaiseTick(0.1);
        Assert.Equal(0x0002, map.CurrentDungeonId);
        Assert.Equal("dungeon/0002", ui.Images.Loaded[1]);
        Assert.Equal(1, ui.Images.ReleaseCount);

        map.SetVisible(false);
        Assert.False(ui.Canvas.IsVisible);
        Assert.False(settings.DungeonMapVisible);
        map.SetVisible(true);
        Assert.True(ui.Canvas.IsVisible);

        fake.PluginNavigation.SnapshotValue = Snapshot(0x00020001, isOutdoor: true);
        fake.PluginEvents.RaiseTick(0.1);
        Assert.False(ui.Canvas.IsVisible);
        Assert.Equal(0, map.CurrentDungeonId);
        Assert.Equal(2, ui.Images.ReleaseCount);
    }

    [Fact]
    public void DungeonImageLoadsWhenTheUiBecomesAvailable()
    {
        var fake = new FakePluginHost();
        fake.PluginNavigation.SnapshotValue = Snapshot(0x00010001, isOutdoor: false);
        var ui = new RecordingUi();
        ui.Images.IsAvailable = false;
        using var map = new GoArrowDungeonMap(
            new CanvasHost(fake, ui), new GoArrowSettings(), DungeonMapTestData.CreateCatalog());

        map.Enable();
        Assert.False(ui.Canvas!.IsVisible);
        Assert.Empty(ui.Images.Loaded);

        ui.Images.IsAvailable = true;
        fake.PluginEvents.RaiseTick(0.1);

        Assert.True(ui.Canvas.IsVisible);
        Assert.Equal("dungeon/0001", Assert.Single(ui.Images.Loaded));
    }

    private static PluginNavigationSnapshot Snapshot(uint cellId, bool isOutdoor) => new(
        true, false, 1, new PluginNavigationPosition(cellId, 0, 0, 0, 0, isOutdoor), false, false);

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
        public NoOpPluginCanvas? Canvas { get; private set; }
        public Action<IPluginPainter>? Paint { get; private set; }
        public RecordingImages Images { get; } = new();
        IPluginImages IUiRegistry.Images => Images;
        public void AddMarkupPanel(string markupPath, object binding) { }
        public IPluginCanvas RegisterCanvas(PluginCanvasDescriptor descriptor, Action<IPluginPainter> paint)
        {
            Canvas = new NoOpPluginCanvas(descriptor);
            Paint = paint;
            return Canvas;
        }
    }

    private sealed class RecordingImages : IPluginImages
    {
        public bool IsAvailable { get; set; } = true;
        public List<string> Loaded { get; } = new();
        public int ReleaseCount { get; private set; }

        public PluginImage FromStream(string name, Func<Stream> open)
        {
            using var stream = open();
            Assert.True(stream.Length > 10);
            Loaded.Add(name);
            return new PluginImage(Loaded.Count, 689, 924);
        }

        public bool Release(PluginImage image)
        {
            ReleaseCount++;
            return true;
        }
    }

    private sealed class RecordingPainter : IPluginPainter
    {
        public int Width => 560;
        public int Height => 620;
        public List<PluginImage> Images { get; } = new();
        public List<PluginRect> ImageRects { get; } = new();
        public List<string> Text { get; } = new();
        public void Clear(PluginColor color) { }
        public void FillRect(PluginRect rect, PluginColor color) { }
        public void StrokeRect(PluginRect rect, PluginColor color, float thickness = 1) { }
        public void DrawLine(PluginPoint from, PluginPoint to, PluginColor color, float thickness = 1) { }
        public void DrawText(string text, PluginPoint position, PluginColor color, bool outline = false) => Text.Add(text);
        public PluginSize MeasureText(string text) => new(0, 0);
        public void DrawImage(PluginImage image, PluginRect destination, PluginColor tint)
        {
            Images.Add(image);
            ImageRects.Add(destination);
        }
        public void DrawImageTransformed(PluginImage image, PluginRect destination,
            PluginColor tint, double rotationRadians, PluginPoint pivot, double scaleX = 1, double scaleY = 1) { }
        public void PushClip(PluginRect rect) { }
        public void PopClip() { }
    }
}
