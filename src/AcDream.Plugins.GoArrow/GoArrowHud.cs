using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow;

/// <summary>Plugin-owned directional arrow and compact toolbar canvases.</summary>
internal sealed class GoArrowHud : IDisposable
{
    private readonly IPluginHost _host;
    private readonly GoArrowDestination _destination;
    private readonly GoArrowNavigator _navigator;
    private readonly GoArrowSettings _settings;
    private IPluginCanvas? _arrow;
    private IPluginCanvas? _toolbar;
    private Action<double>? _tick;
    private PluginPoint? _arrowDrag;
    private PluginPoint? _toolbarDrag;
    private bool _toolbarDragging;
    private PluginPoint _arrowDisplayedOffset;
    private PluginPoint _toolbarDisplayedOffset;
    private bool _positionDirty;

    public GoArrowHud(IPluginHost host, GoArrowDestination destination, GoArrowNavigator navigator, GoArrowSettings settings)
    {
        _host = host;
        _destination = destination;
        _navigator = navigator;
        _settings = settings;
    }

    public void Enable()
    {
        if (!_host.HasUi || _arrow is not null)
            return;

        _arrow = _host.Ui.RegisterCanvas(
            new PluginCanvasDescriptor("goarrow.arrow", 260, 90)
            {
                Anchor = PluginCanvasAnchor.TopRight,
                Offset = new PluginPoint(_settings.ArrowOffsetX, _settings.ArrowOffsetY),
                StartVisible = _settings.HudVisible,
                AcceptsPointerInput = !_settings.HudClickThrough,
            }, PaintArrow);
        _toolbar = _host.Ui.RegisterCanvas(
            new PluginCanvasDescriptor("goarrow.toolbar", 260, 35)
            {
                Anchor = PluginCanvasAnchor.TopRight,
                Offset = new PluginPoint(_settings.ToolbarOffsetX, _settings.ToolbarOffsetY),
                StartVisible = _settings.ToolbarVisible,
                AcceptsPointerInput = !_settings.HudClickThrough,
            }, PaintToolbar);
        if (!_settings.HudClickThrough)
        {
            _arrow.PointerHandler = OnArrowInput;
            _toolbar.PointerHandler = OnToolbarInput;
        }
        _tick = _ =>
        {
            // The host lays out canvases before raising the plugin tick. Keep
            // the offset that pointer-local coordinates are measured against.
            _arrowDisplayedOffset = _arrow.Offset;
            _toolbarDisplayedOffset = _toolbar.Offset;
            if (_positionDirty)
            {
                _positionDirty = false;
                _settings.Save(_host.Storage);
            }
            _arrow.Invalidate();
            _toolbar.Invalidate();
        };
        _host.Events.Tick += _tick;
        _arrow.Invalidate();
        _toolbar.Invalidate();
    }

    private void OnArrowInput(PluginPointerEvent input)
    {
        HandleDrag(_arrow, input, ref _arrowDrag, ref _arrowDisplayedOffset, offset =>
        {
            _settings.ArrowOffsetX = offset.X;
            _settings.ArrowOffsetY = offset.Y;
        });
    }

    private void OnToolbarInput(PluginPointerEvent input)
    {
        if (input.Kind == PluginPointerEventKind.Down && input.Button == PluginPointerButton.Left)
        {
            _toolbarDragging = false;
            HandleToolbarDrag(input);
            return;
        }
        if (_toolbarDrag is not { } origin)
            return;
        if (input.Kind == PluginPointerEventKind.Cancelled)
        {
            if (_toolbarDragging)
                HandleToolbarDrag(input);
            else
                _toolbarDrag = null;
            _toolbarDragging = false;
            return;
        }
        if (input.Kind is not (PluginPointerEventKind.Move or PluginPointerEventKind.Up))
            return;
        double dx = input.Position.X - origin.X;
        double dy = input.Position.Y - origin.Y;
        _toolbarDragging |= dx * dx + dy * dy >= 16;
        if (_toolbarDragging)
            HandleToolbarDrag(input);
        else if (input.Kind == PluginPointerEventKind.Up)
        {
            _toolbarDrag = null;
            if (input.Button == PluginPointerButton.Left
                && input.Position.X >= 0 && input.Position.X < 260
                && input.Position.Y >= 0 && input.Position.Y < 35
                && (origin.X < 130) == (input.Position.X < 130))
            {
                if (input.Position.X < 130)
                    _navigator.StopNavigation();
                else
                    _navigator.ResumeAfterInteraction();
            }
        }
        if (input.Kind == PluginPointerEventKind.Up)
            _toolbarDragging = false;
    }

    private void HandleToolbarDrag(PluginPointerEvent input) =>
        HandleDrag(_toolbar, input, ref _toolbarDrag, ref _toolbarDisplayedOffset, offset =>
        {
            _settings.ToolbarOffsetX = offset.X;
            _settings.ToolbarOffsetY = offset.Y;
        });

    private void HandleDrag(IPluginCanvas? canvas, PluginPointerEvent input,
        ref PluginPoint? start, ref PluginPoint displayedOffset, Action<PluginPoint> saveOffset)
    {
        if (canvas is null)
            return;
        if (input.Kind == PluginPointerEventKind.Down && input.Button == PluginPointerButton.Left)
        {
            start = input.Position;
            displayedOffset = canvas.Offset;
        }
        else if (input.Kind is PluginPointerEventKind.Move or PluginPointerEventKind.Up
            && start is { } origin)
        {
            var offset = new PluginPoint(
                displayedOffset.X + input.Position.X - origin.X,
                displayedOffset.Y + input.Position.Y - origin.Y);
            canvas.Offset = offset;
            if (input.Kind == PluginPointerEventKind.Up)
            {
                start = null;
                saveOffset(offset);
                _positionDirty = true;
            }
        }
        else if (input.Kind == PluginPointerEventKind.Cancelled && start is not null)
        {
            start = null;
            saveOffset(canvas.Offset);
            _positionDirty = true;
        }
    }

    private void PaintArrow(IPluginPainter painter)
    {
        painter.Clear(PluginColor.Transparent);
        painter.FillRect(new PluginRect(0, 0, painter.Width, painter.Height), new PluginColor(12, 20, 29, 226));
        painter.StrokeRect(new PluginRect(0, 0, painter.Width, painter.Height), new PluginColor(111, 128, 138, 215));
        painter.FillRect(new PluginRect(72, 10, 1, painter.Height - 20), new PluginColor(103, 120, 130, 130));

        var center = new PluginPoint(37, 45);
        DrawCompassDial(painter, center);

        var snapshot = _host.Automation.Navigation.Snapshot;
        if (snapshot.IsAvailable && (snapshot.IsPortalSpace || !snapshot.Position.IsOutdoor))
        {
            painter.DrawText("OUTDOOR ROUTE PAUSED", new PluginPoint(84, 25),
                new PluginColor(255, 215, 113), outline: true);
            painter.DrawText("Resumes outdoors", new PluginPoint(84, 53), PluginColor.White,
                outline: true);
            return;
        }

        double bearingDegrees = _destination.BearingDegrees;
        double headingDegrees = snapshot.Position.HeadingDegrees;
        if (double.IsFinite(bearingDegrees) && double.IsFinite(headingDegrees))
            DrawPointer(painter, center, (bearingDegrees - headingDegrees) * Math.PI / 180.0);

        string name = _destination.GetImmediateTarget()?.Name ?? "No destination";
        string bearing = double.IsFinite(bearingDegrees) ? $"{bearingDegrees:0}°" : "--°";
        string distance = TravelDistance.Format(_destination.GuidanceDistance);
        string readout = (_settings.ShowBearing, _settings.ShowDistance) switch
        {
            (true, true) => $"{bearing}  ·  {distance}",
            (true, false) => bearing,
            (false, true) => distance,
            _ => string.Empty,
        };
        painter.DrawText("NEXT WAYPOINT", new PluginPoint(84, 8), new PluginColor(156, 177, 186), outline: true);
        painter.DrawText(FitText(painter, name, painter.Width - 94),
            new PluginPoint(84, 31), PluginColor.White, outline: true);
        if (readout.Length > 0)
            painter.DrawText(readout, new PluginPoint(84, 57),
                new PluginColor(255, 215, 113), outline: true);
    }

    private static void DrawCompassDial(IPluginPainter painter, PluginPoint center)
    {
        var ring = new PluginColor(107, 129, 139, 170);
        const int segments = 32;
        for (int i = 0; i < segments; i++)
        {
            double a = i * 2.0 * Math.PI / segments;
            double b = (i + 1) * 2.0 * Math.PI / segments;
            painter.DrawLine(
                new PluginPoint(center.X + Math.Sin(a) * 29, center.Y - Math.Cos(a) * 29),
                new PluginPoint(center.X + Math.Sin(b) * 29, center.Y - Math.Cos(b) * 29), ring, 1.5f);
        }
        for (int i = 0; i < 4; i++)
        {
            double a = i * Math.PI / 2.0;
            painter.DrawLine(
                new PluginPoint(center.X + Math.Sin(a) * 31, center.Y - Math.Cos(a) * 31),
                new PluginPoint(center.X + Math.Sin(a) * 34, center.Y - Math.Cos(a) * 34),
                new PluginColor(190, 206, 211, 210), 2);
        }
        painter.FillRect(new PluginRect(center.X - 2, center.Y - 2, 4, 4),
            new PluginColor(255, 224, 145));
    }

    private static void DrawPointer(IPluginPainter painter, PluginPoint center, double angle)
    {
        double dx = Math.Sin(angle);
        double dy = -Math.Cos(angle);
        double px = -dy;
        double py = dx;
        PluginPoint At(double forward, double sideways = 0) => new(
            center.X + dx * forward + px * sideways,
            center.Y + dy * forward + py * sideways);

        PluginPoint tail = At(-16);
        PluginPoint tip = At(23);
        PluginPoint left = At(7, -10);
        PluginPoint right = At(7, 10);
        var outline = new PluginColor(30, 22, 11, 240);
        var gold = new PluginColor(245, 175, 52);
        var highlight = new PluginColor(255, 239, 170);
        painter.DrawLine(tail, tip, outline, 9);
        painter.DrawLine(left, tip, outline, 9);
        painter.DrawLine(right, tip, outline, 9);
        painter.DrawLine(tail, tip, gold, 5);
        painter.DrawLine(left, tip, gold, 5);
        painter.DrawLine(right, tip, gold, 5);
        painter.DrawLine(At(-12), At(13), highlight, 1.5f);
    }

    private static string FitText(IPluginPainter painter, string text, double width)
    {
        if (painter.MeasureText(text).Width <= width)
            return text;
        while (text.Length > 0 && painter.MeasureText(text + "…").Width > width)
            text = text[..^1];
        return text + "…";
    }

    private static void PaintToolbar(IPluginPainter painter)
    {
        painter.Clear(PluginColor.Transparent);
        painter.FillRect(new PluginRect(0, 0, painter.Width, painter.Height), new PluginColor(0, 0, 0, 175));
        painter.StrokeRect(new PluginRect(0, 0, painter.Width, painter.Height), new PluginColor(128, 128, 128));
        painter.DrawLine(new PluginPoint(130, 0), new PluginPoint(130, 35), new PluginColor(128, 128, 128));
        painter.DrawText("Stop", new PluginPoint(48, 9), PluginColor.White);
        painter.DrawText("Resume", new PluginPoint(166, 9), PluginColor.White);
    }

    public void ResetPositions()
    {
        _arrowDrag = null;
        _toolbarDrag = null;
        _toolbarDragging = false;
        if (_arrow is not null)
            _arrow.Offset = new PluginPoint(_settings.ArrowOffsetX, _settings.ArrowOffsetY);
        if (_toolbar is not null)
            _toolbar.Offset = new PluginPoint(_settings.ToolbarOffsetX, _settings.ToolbarOffsetY);
        _arrowDisplayedOffset = _arrow?.Offset ?? default;
        _toolbarDisplayedOffset = _toolbar?.Offset ?? default;
    }

    public void Dispose()
    {
        if (_positionDirty)
            _settings.Save(_host.Storage);
        if (_tick is not null)
            _host.Events.Tick -= _tick;
        _tick = null;
        if (_arrow is not null)
            _arrow.PointerHandler = null;
        if (_toolbar is not null)
            _toolbar.PointerHandler = null;
        _arrow?.Dispose();
        _toolbar?.Dispose();
        _arrow = null;
        _toolbar = null;
    }
}
