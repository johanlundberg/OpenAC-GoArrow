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
        painter.FillRect(new PluginRect(0, 0, painter.Width, painter.Height), new PluginColor(0, 0, 0, 175));
        painter.StrokeRect(new PluginRect(0, 0, painter.Width, painter.Height), new PluginColor(128, 128, 128));

        string name = _destination.GetImmediateTarget()?.Name ?? "No destination";
        string bearing = double.IsNaN(_destination.BearingDegrees) ? "--" : $"{_destination.BearingDegrees:0}°";
        string distance = double.IsNaN(_destination.GuidanceDistance) ? "--" : $"{_destination.GuidanceDistance:0.0}u";
        var color = new PluginColor(255, 220, 80);
        if (!double.IsNaN(_destination.BearingDegrees))
        {
            double heading = _host.Automation.Navigation.Snapshot.Position.HeadingDegrees;
            double angle = (_destination.BearingDegrees - heading) * (Math.PI / 180.0);
            double dx = Math.Sin(angle);
            double dy = -Math.Cos(angle);
            var center = new PluginPoint(32, 43);
            var tip = new PluginPoint(center.X + dx * 22, center.Y + dy * 22);
            var tail = new PluginPoint(center.X - dx * 14, center.Y - dy * 14);
            painter.DrawLine(tail, tip, color, 3);
            painter.DrawLine(tip, new PluginPoint(
                tip.X - Math.Sin(angle - 0.6) * 11,
                tip.Y + Math.Cos(angle - 0.6) * 11), color, 3);
            painter.DrawLine(tip, new PluginPoint(
                tip.X - Math.Sin(angle + 0.6) * 11,
                tip.Y + Math.Cos(angle + 0.6) * 11), color, 3);
        }
        painter.DrawText($"{bearing}  {distance}", new PluginPoint(66, 21), color, outline: true);
        painter.DrawText($"Next: {name}", new PluginPoint(66, 50), PluginColor.White, outline: true);
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
