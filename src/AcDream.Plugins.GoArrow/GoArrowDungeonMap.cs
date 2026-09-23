using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow;

/// <summary>Displays the matching schematic dungeon image while indoors.</summary>
internal sealed class GoArrowDungeonMap : IDisposable
{
    private const int CanvasWidth = 560;
    private const int CanvasHeight = 620;
    private const int HeaderHeight = 42;
    private readonly IPluginHost _host;
    private readonly GoArrowSettings _settings;
    private readonly DungeonMapCatalog _catalog;
    private IPluginCanvas? _canvas;
    private PluginImage _image;
    private DungeonMapEntry? _currentMap;
    private int _selectedId;
    private bool _pendingImage;
    private Action<double>? _tick;
    private double _zoom = 1;
    private double _panX;
    private double _panY;
    private PluginPoint? _lastDrag;
    private PluginPoint? _windowDrag;
    private PluginPoint _displayedOffset;
    private bool _positionDirty;
    private double _elevation;

    public GoArrowDungeonMap(IPluginHost host, GoArrowSettings settings, DungeonMapCatalog catalog)
    {
        _host = host;
        _settings = settings;
        _catalog = catalog;
    }

    internal int CurrentDungeonId => _currentMap?.Id ?? 0;
    internal string CurrentMapName => _currentMap?.Name ?? string.Empty;

    public void Enable()
    {
        if (!_host.HasUi || _canvas is not null)
            return;

        _canvas = _host.Ui.RegisterCanvas(
            new PluginCanvasDescriptor("goarrow.dungeon", CanvasWidth, CanvasHeight)
            {
                Anchor = PluginCanvasAnchor.TopLeft,
                Offset = new PluginPoint(_settings.DungeonOffsetX, _settings.DungeonOffsetY),
                StartVisible = false,
                AcceptsPointerInput = true,
            },
            Paint
        );
        _canvas.PointerHandler = OnInput;
        _tick = _ =>
        {
            _displayedOffset = _canvas.Offset;
            if (_positionDirty)
            {
                _positionDirty = false;
                _settings.Save(_host.Storage);
            }
            Refresh();
        };
        _host.Events.Tick += _tick;
        Refresh();
    }

    public void SetVisible(bool visible)
    {
        _settings.DungeonMapVisible = visible;
        _settings.Save(_host.Storage);
        Refresh();
    }

    public void ResetPosition()
    {
        _windowDrag = null;
        if (_canvas is not null)
            _canvas.Offset = new PluginPoint(_settings.DungeonOffsetX, _settings.DungeonOffsetY);
        _displayedOffset = _canvas?.Offset ?? default;
    }

    private void Refresh()
    {
        if (_canvas is null)
            return;

        var snapshot = _host.Automation.Navigation.Snapshot;
        var position = snapshot.Position;
        int id =
            _host.Automation.IsAvailable
            && snapshot.IsAvailable
            && !snapshot.IsPortalSpace
            && !position.IsOutdoor
                ? (int)(position.CellId >> 16)
                : 0;
        if (id != _selectedId)
            SelectDungeon(id);
        if (_image.IsValid && !_host.Ui.Images.IsAvailable)
        {
            _image = PluginImage.None;
            _pendingImage = _currentMap is not null;
        }
        if (_pendingImage && _host.Ui.Images.IsAvailable && _currentMap is { } pending)
            LoadImage(pending);

        bool show = _settings.DungeonMapVisible && _image.IsValid && id != 0;
        if (_canvas.IsVisible != show)
            _canvas.IsVisible = show;
        if (!show)
            return;

        if (_elevation != position.Elevation)
        {
            _elevation = position.Elevation;
            _canvas.Invalidate();
        }
    }

    private void SelectDungeon(int id)
    {
        _selectedId = id;
        if (_image.IsValid)
            _host.Ui.Images.Release(_image);
        _image = PluginImage.None;
        _currentMap = null;
        _pendingImage = false;
        _zoom = 1;
        _panX = 0;
        _panY = 0;

        if (id == 0 || !_catalog.TryGet(id, out DungeonMapEntry map))
            return;

        _currentMap = map;
        if (!_host.Ui.Images.IsAvailable)
        {
            _pendingImage = true;
            return;
        }
        LoadImage(map);
    }

    private void LoadImage(DungeonMapEntry map)
    {
        _pendingImage = false;
        _image = _host.Ui.Images.FromStream($"dungeon/{map.Id:X4}", () => _catalog.OpenImage(map));
        if (!_image.IsValid)
            _host.Log.Warn($"GoArrow: Dungeon map {map.Id:X4} could not be loaded.");
        _canvas?.Invalidate();
    }

    private void Paint(IPluginPainter painter)
    {
        painter.Clear(PluginColor.Transparent);
        painter.FillRect(
            new PluginRect(0, 0, CanvasWidth, CanvasHeight),
            new PluginColor(18, 18, 18, 235)
        );
        painter.StrokeRect(
            new PluginRect(0, 0, CanvasWidth, CanvasHeight),
            new PluginColor(130, 130, 130)
        );
        painter.DrawText(
            $"{CurrentMapName} ({CurrentDungeonId:X4})  ·  Drag title to move",
            new PluginPoint(12, 10),
            PluginColor.White
        );
        painter.DrawText("×", new PluginPoint(CanvasWidth - 24, 10), PluginColor.White);
        if (!_image.IsValid)
            return;

        const double margin = 10;
        double areaWidth = CanvasWidth - 2 * margin;
        double areaHeight = CanvasHeight - HeaderHeight - margin;
        double scale = Math.Min(areaWidth / _image.Width, areaHeight / _image.Height) * _zoom;
        double width = _image.Width * scale;
        double height = _image.Height * scale;
        double x = margin + (areaWidth - width) / 2 + _panX;
        double y = HeaderHeight + (areaHeight - height) / 2 + _panY;
        painter.PushClip(new PluginRect(margin, HeaderHeight, areaWidth, areaHeight));
        painter.DrawImage(_image, new PluginRect(x, y, width, height), PluginColor.White);
        painter.PopClip();
        painter.DrawText(
            $"Elevation: {_elevation * 240:0} m   Wheel: zoom   Drag: pan",
            new PluginPoint(12, CanvasHeight - 20),
            new PluginColor(245, 225, 130),
            outline: true
        );
    }

    private void OnInput(PluginPointerEvent input)
    {
        if (_canvas is null)
            return;
        if (input.Kind == PluginPointerEventKind.Down && input.Button == PluginPointerButton.Left)
        {
            if (input.Position.Y < HeaderHeight && input.Position.X > CanvasWidth - 40)
            {
                SetVisible(false);
                return;
            }
            if (input.Position.Y < HeaderHeight)
            {
                _windowDrag = input.Position;
                _displayedOffset = _canvas.Offset;
            }
            else
                _lastDrag = input.Position;
        }
        else if (input.Kind == PluginPointerEventKind.Move && _windowDrag is { } origin)
        {
            _canvas.Offset = new PluginPoint(
                _displayedOffset.X + input.Position.X - origin.X,
                _displayedOffset.Y + input.Position.Y - origin.Y
            );
        }
        else if (input.Kind == PluginPointerEventKind.Move && _lastDrag is { } last)
        {
            _panX += input.Position.X - last.X;
            _panY += input.Position.Y - last.Y;
            _lastDrag = input.Position;
            _canvas.Invalidate();
        }
        else if (input.Kind is PluginPointerEventKind.Up or PluginPointerEventKind.Cancelled)
        {
            if (_windowDrag is { } dragStart)
            {
                var offset =
                    input.Kind == PluginPointerEventKind.Up
                        ? new PluginPoint(
                            _displayedOffset.X + input.Position.X - dragStart.X,
                            _displayedOffset.Y + input.Position.Y - dragStart.Y
                        )
                        : _canvas.Offset;
                _canvas.Offset = offset;
                _settings.DungeonOffsetX = offset.X;
                _settings.DungeonOffsetY = offset.Y;
                _positionDirty = true;
            }
            _windowDrag = null;
            _lastDrag = null;
        }
        else if (input.Kind == PluginPointerEventKind.Wheel)
        {
            _zoom = Math.Clamp(_zoom * (input.WheelDelta > 0 ? 1.2 : 1 / 1.2), 1, 8);
            _canvas.Invalidate();
        }
    }

    public void Dispose()
    {
        if (_positionDirty)
            _settings.Save(_host.Storage);
        if (_tick is not null)
            _host.Events.Tick -= _tick;
        _tick = null;
        if (_canvas is not null)
            _canvas.PointerHandler = null;
        _canvas?.Dispose();
        _canvas = null;
        if (_image.IsValid)
            _host.Ui.Images.Release(_image);
        _image = PluginImage.None;
        _currentMap = null;
    }
}
