using AcDream.Plugin.Abstractions;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow;

/// <summary>Plugin-owned world map surface with route and navigation markers.</summary>
internal sealed class GoArrowMap : IDisposable
{
    private readonly IPluginHost _host;
    private readonly GoArrowDestination _destination;
    private readonly GoArrowSettings _settings;
    private IPluginMapSurface? _map;
    private Action<double>? _tick;

    public GoArrowMap(IPluginHost host, GoArrowDestination destination, GoArrowSettings settings)
    {
        _host = host;
        _destination = destination;
        _settings = settings;
    }

    public void Enable()
    {
        if (!_host.HasUi || _map is not null)
            return;
        var bounds = new PluginMapViewport(
            new PluginMapPoint(_settings.MapCenterEastWest, _settings.MapCenterNorthSouth),
            _settings.MapWidth, _settings.MapHeight);
        _map = _host.Maps.AddMap("goarrow.dereth", bounds);
        _map.Input += OnInput;
        _tick = _ => Refresh();
        _host.Events.Tick += _tick;
        Refresh();
    }

    private void OnInput(PluginMapInput input)
    {
        if (_map is null)
            return;
        if (input.Kind == PluginMapInputKind.Wheel)
        {
            double factor = input.WheelDelta > 0 ? 0.9 : 1.1;
            var view = _map.Viewport;
            _map.Viewport = new PluginMapViewport(view.Center, view.Width * factor, view.Height * factor);
            PersistViewport();
            return;
        }
        if (input.Kind != PluginMapInputKind.Click || _map.Background is null)
            return;
        PluginMapPoint point = _map.Background.Coordinates.PixelToWorld(
            input.Position, _map.Background.PixelWidth, _map.Background.PixelHeight);
        _destination.SetCoordinate(point.NorthSouth, point.EastWest,
            $"{point.NorthSouth:0.###}N {point.EastWest:0.###}E");
        PersistViewport();
    }

    private void PersistViewport()
    {
        if (_map is null) return;
        var view = _map.Viewport;
        _settings.MapCenterEastWest = view.Center.EastWest;
        _settings.MapCenterNorthSouth = view.Center.NorthSouth;
        _settings.MapWidth = view.Width;
        _settings.MapHeight = view.Height;
        _settings.Save(_host.Storage);
    }

    private void Refresh()
    {
        if (_map is null)
            return;
        var markers = new List<PluginMapMarker>();
        if (_host.Automation.IsAvailable)
        {
            var position = _host.Automation.Navigation.Snapshot.Position;
            markers.Add(new PluginMapMarker("current",
                new PluginMapPoint(position.EastWest, position.NorthSouth), "Current position", IsSelected: true));
        }
        if (_destination.TargetLocation is { } target)
            markers.Add(new PluginMapMarker("destination",
                new PluginMapPoint(target.Coords.EW, target.Coords.NS), target.Name, IsSelected: true));
        _map.SetMarkers(markers);
        var points = _destination.CurrentRoute?.Steps
            .SelectMany(step => new[]
            {
                new PluginMapPoint(step.From.Coords.EW, step.From.Coords.NS),
                new PluginMapPoint(step.To.Coords.EW, step.To.Coords.NS)
            }).ToArray() ?? Array.Empty<PluginMapPoint>();
        _map.SetRoute(points);
    }

    public void Dispose()
    {
        if (_tick is not null) _host.Events.Tick -= _tick;
        _tick = null;
        if (_map is not null) _map.Input -= OnInput;
        _map?.Dispose();
        _map = null;
    }
}
