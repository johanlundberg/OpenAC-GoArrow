using AcDream.Plugin.Abstractions;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow;

/// <summary>Plugin-owned world map surface with route and navigation markers.</summary>
internal sealed class GoArrowMap : IDisposable
{
    private readonly IPluginHost _host;
    private readonly GoArrowDestination _destination;
    private IPluginMapSurface? _map;
    private Action<double>? _tick;

    public GoArrowMap(IPluginHost host, GoArrowDestination destination)
    {
        _host = host;
        _destination = destination;
    }

    public void Enable()
    {
        if (!_host.HasUi || _map is not null)
            return;
        var bounds = new PluginMapViewport(new PluginMapPoint(0, 0), 1000, 1000);
        _map = _host.Maps.AddMap("goarrow.dereth", bounds);
        _map.Input += OnInput;
        _tick = _ => Refresh();
        _host.Events.Tick += _tick;
        Refresh();
    }

    private void OnInput(PluginMapInput input)
    {
        if (_map is null || input.Kind != PluginMapInputKind.Click || _map.Background is null)
            return;
        PluginMapPoint point = _map.Background.Coordinates.PixelToWorld(
            input.Position, _map.Background.PixelWidth, _map.Background.PixelHeight);
        _destination.SetCoordinate(point.NorthSouth, point.EastWest,
            $"{point.NorthSouth:0.###}N {point.EastWest:0.###}E");
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
