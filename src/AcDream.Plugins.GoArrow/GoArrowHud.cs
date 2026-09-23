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
                Offset = new PluginPoint(-70, 120),
                StartVisible = _settings.HudVisible,
                AcceptsPointerInput = !_settings.HudClickThrough,
            }, PaintArrow);
        _toolbar = _host.Ui.RegisterCanvas(
            new PluginCanvasDescriptor("goarrow.toolbar", 260, 35)
            {
                Anchor = PluginCanvasAnchor.TopRight,
                Offset = new PluginPoint(-70, 215),
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
            _arrow.Invalidate();
            _toolbar.Invalidate();
        };
        _host.Events.Tick += _tick;
        _arrow.Invalidate();
        _toolbar.Invalidate();
    }

    private void OnArrowInput(PluginPointerEvent input)
    {
        if (input.Kind == PluginPointerEventKind.Up && input.Button == PluginPointerButton.Left)
            _navigator.StopNavigation();
    }

    private void OnToolbarInput(PluginPointerEvent input)
    {
        if (input.Kind != PluginPointerEventKind.Up || input.Button != PluginPointerButton.Left
            || input.Position.Y < 0 || input.Position.Y >= 35)
            return;
        if (input.Position.X is >= 0 and < 90)
            _navigator.StopNavigation();
        else if (input.Position.X is >= 90 and < 180)
            _navigator.ResumeAfterInteraction();
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
        string distance = double.IsFinite(_destination.GuidanceDistance)
            ? $"{_destination.GuidanceDistance:0.0}u" : "--u";
        painter.DrawText("NEXT WAYPOINT", new PluginPoint(84, 8), new PluginColor(156, 177, 186), outline: true);
        painter.DrawText(FitText(painter, name, painter.Width - 94),
            new PluginPoint(84, 31), PluginColor.White, outline: true);
        painter.DrawText($"{bearing}  ·  {distance}", new PluginPoint(84, 57),
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
        painter.DrawLine(tail, At(8), outline, 12);
        painter.DrawLine(left, tip, outline, 12);
        painter.DrawLine(right, tip, outline, 12);
        painter.DrawLine(tail, At(8), gold, 7);
        painter.DrawLine(left, tip, gold, 7);
        painter.DrawLine(right, tip, gold, 7);
        painter.DrawLine(At(-13), At(6), highlight, 2);
        painter.DrawLine(At(9, -7), tip, highlight, 2);
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
        painter.DrawLine(new PluginPoint(90, 0), new PluginPoint(90, 35), new PluginColor(128, 128, 128));
        painter.DrawText("Stop", new PluginPoint(24, 9), PluginColor.White);
        painter.DrawText("Resume", new PluginPoint(108, 9), PluginColor.White);
    }

    public void Dispose()
    {
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
