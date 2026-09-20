using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow;

/// <summary>Plugin-owned directional arrow and compact toolbar HUD.</summary>
internal sealed class GoArrowHud : IDisposable
{
    private readonly IPluginHost _host;
    private readonly GoArrowDestination _destination;
    private readonly GoArrowNavigator _navigator;
    private IPluginHudRegistration? _arrow;
    private IPluginHudRegistration? _toolbar;
    private Action<double>? _tick;

    public GoArrowHud(IPluginHost host, GoArrowDestination destination, GoArrowNavigator navigator)
    {
        _host = host;
        _destination = destination;
        _navigator = navigator;
    }

    public void Enable()
    {
        if (!_host.HasUi || _arrow is not null)
            return;
        _arrow = _host.Rendering.AddHud(new PluginHudDescriptor(
            "goarrow.arrow", "GoArrow", new PluginHudBounds(20, 120, 260, 90), true,
            Movable: true, Resizable: true, ClickThrough: false, Layer: 10));
        _toolbar = _host.Rendering.AddHud(new PluginHudDescriptor(
            "goarrow.toolbar", "GoArrow controls", new PluginHudBounds(20, 215, 260, 35), true,
            Movable: true, Resizable: false, ClickThrough: false, Layer: 10));
        _arrow.Input += OnArrowInput;
        _toolbar.Input += OnToolbarInput;
        _tick = _ => Render();
        _host.Events.Tick += _tick;
    }

    private void OnArrowInput(PluginHudInput input)
    {
        if (input.Kind == PluginHudInputKind.Click && input.Button == PluginMouseButton.Left)
            _navigator.StopNavigation();
    }

    private void OnToolbarInput(PluginHudInput input)
    {
        if (input.Kind != PluginHudInputKind.Click || input.Button != PluginMouseButton.Left)
            return;
        // The compact toolbar is deliberately action-oriented and does not own game input.
        if (input.Position.X < 55) _navigator.StopNavigation();
        else if (input.Position.X < 110) _navigator.ResumeAfterInteraction();
    }

    private void Render()
    {
        if (_arrow is null || !_arrow.IsVisible)
            return;
        IPluginRenderSurface surface = _arrow.Surface;
        surface.BeginFrame();
        string name = _destination.TargetName;
        string bearing = double.IsNaN(_destination.BearingDegrees) ? "--" : $"{_destination.BearingDegrees:0}°";
        string distance = double.IsNaN(_destination.EstimatedDistance) ? "--" : $"{_destination.EstimatedDistance:0.0}u";
        surface.DrawText($"➤ {bearing}  {distance}", new PluginPoint(12, 28),
            new PluginTextStyle("sans", 22, new PluginColor(255, 220, 80), true));
        surface.DrawText(string.IsNullOrEmpty(name) ? "No destination" : name,
            new PluginPoint(12, 58), new PluginTextStyle("sans", 14, new PluginColor(255, 255, 255)));
        surface.EndFrame();
        if (_toolbar is not null && _toolbar.IsVisible)
        {
            _toolbar.Surface.BeginFrame();
            _toolbar.Surface.DrawText("Stop     Resume", new PluginPoint(10, 20),
                new PluginTextStyle("sans", 14, new PluginColor(220, 220, 220)));
            _toolbar.Surface.EndFrame();
        }
    }

    public void Dispose()
    {
        if (_tick is not null) _host.Events.Tick -= _tick;
        _tick = null;
        if (_arrow is not null) _arrow.Input -= OnArrowInput;
        if (_toolbar is not null) _toolbar.Input -= OnToolbarInput;
        _arrow?.Dispose();
        _toolbar?.Dispose();
        _arrow = null;
        _toolbar = null;
    }
}
