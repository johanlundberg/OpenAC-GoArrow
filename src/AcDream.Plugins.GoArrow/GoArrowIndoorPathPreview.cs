using System.Diagnostics;
using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow;

/// <summary>Displays a plan-only path to the current indoor destination.</summary>
internal sealed class GoArrowIndoorPathPreview
{
    private const int MaximumDisplayedWaypoints = 200;
    private readonly IPluginHost _host;
    private readonly GoArrowNavigator _navigator;
    private readonly GoArrowSettings _settings;
    private readonly Func<uint, float, Task<PluginNavigationPlan>> _previewObject;
    private readonly Func<PluginNavigationPosition, float, Task<PluginNavigationPlan>>
        _previewPosition;
    private PreviewTarget? _target;
    private PluginNavigationPosition _requestOrigin;
    private PluginNavigationPosition _resultOrigin;
    private Task<PluginNavigationPlan>? _pending;
    private long _lastRequestAt;
    private bool _hasResult;

    private readonly record struct PreviewTarget(
        uint PlayerId,
        uint ObjectId,
        PluginNavigationPosition Position
    );

    public GoArrowIndoorPathPreview(
        IPluginHost host,
        GoArrowNavigator navigator,
        GoArrowSettings settings,
        Func<uint, float, Task<PluginNavigationPlan>>? previewObject = null,
        Func<PluginNavigationPosition, float, Task<PluginNavigationPlan>>? previewPosition = null
    )
    {
        _host = host;
        _navigator = navigator;
        _settings = settings;
        _previewObject = previewObject ?? host.Automation.Navigation.PreviewPathAsync;
        _previewPosition = previewPosition ?? host.Automation.Navigation.PreviewPathAsync;
    }

    public bool Visible { get; private set; }
    public string Summary { get; private set; } = string.Empty;
    public IReadOnlyList<string> Waypoints { get; private set; } = [];

    /// <summary>Call only on the host Tick thread, including when polling completion.</summary>
    public void OnTick()
    {
        if (
            !_host.HasUi
            || !_host.Automation.IsAvailable
            || !_navigator.TryGetIndoorPreviewTarget(out uint objectId, out var position)
        )
        {
            Clear();
            return;
        }

        var snapshot = _host.Automation.Navigation.Snapshot;
        var target = new PreviewTarget(snapshot.LocalObjectId, objectId, position);
        if (_target is not { } previous || !SameTarget(previous, target))
        {
            bool sameMovingObject = _target is { ObjectId: > 0 } old
                && old.PlayerId == target.PlayerId
                && old.ObjectId == target.ObjectId;
            _target = target;
            ObserveDiscardedTask();
            _pending = null;
            _hasResult = false;
            if (!sameMovingObject)
                _lastRequestAt = 0;
            Waypoints = [];
            Summary = "Planning indoor path...";
        }
        Visible = true;

        if (_pending is { IsCompleted: true } completed)
        {
            _pending = null;
            if (SameOrigin(snapshot.Position, _requestOrigin))
            {
                try
                {
                    SetResult(completed.GetAwaiter().GetResult());
                }
                catch (Exception exception)
                {
                    Summary = $"Indoor path preview failed: {exception.Message}";
                    Waypoints = [];
                }
                _resultOrigin = _requestOrigin;
                _hasResult = true;
            }
        }

        if (_pending is not null)
            return;
        if (_hasResult && SameOrigin(snapshot.Position, _resultOrigin))
            return;
        if (
            _lastRequestAt != 0
            && Stopwatch.GetElapsedTime(_lastRequestAt).TotalSeconds < 3
        )
            return;

        _requestOrigin = snapshot.Position;
        _lastRequestAt = Stopwatch.GetTimestamp();
        _hasResult = false;
        Waypoints = [];
        Summary = "Planning indoor path...";
        float arrival = (float)_settings.ArrivalDistance;
        try
        {
            _pending = objectId != 0
                ? _previewObject(objectId, arrival)
                : _previewPosition(position, arrival);
        }
        catch (Exception exception)
        {
            Summary = $"Indoor path preview failed: {exception.Message}";
            _hasResult = true;
            _resultOrigin = _requestOrigin;
        }
    }

    public void Clear()
    {
        _target = null;
        ObserveDiscardedTask();
        _pending = null;
        _hasResult = false;
        _lastRequestAt = 0;
        Visible = false;
        Summary = string.Empty;
        Waypoints = [];
    }

    private void ObserveDiscardedTask()
    {
        if (_pending is not { } pending)
            return;
        _ = pending.ContinueWith(
            completed => _ = completed.Exception,
            TaskContinuationOptions.OnlyOnFaulted | TaskContinuationOptions.ExecuteSynchronously
        );
    }

    private static bool SameTarget(PreviewTarget first, PreviewTarget second) =>
        first.PlayerId == second.PlayerId
        && first.ObjectId == second.ObjectId
        && first.Position.CellId == second.Position.CellId
        && first.Position.HorizontalDistanceMeters(second.Position) < 2
        && SameHeight(first.Position, second.Position);

    private static bool SameOrigin(
        PluginNavigationPosition first,
        PluginNavigationPosition second
    ) =>
        first.CellId == second.CellId
        && first.HorizontalDistanceMeters(second) < 10
        && SameHeight(first, second);

    private static bool SameHeight(
        PluginNavigationPosition first,
        PluginNavigationPosition second
    ) =>
        first.Elevation.Equals(second.Elevation)
        || Math.Abs(first.Elevation - second.Elevation) * 240 < 2;

    private void SetResult(PluginNavigationPlan plan)
    {
        if (plan.Status != PluginNavigationPlanStatus.Routed)
        {
            Summary = plan.Status == PluginNavigationPlanStatus.NoRoute
                ? "No indoor path found."
                : $"Indoor path unavailable ({plan.Status}).";
            if (!string.IsNullOrWhiteSpace(plan.Reason))
                Summary += $" {plan.Reason}";
            Waypoints = [];
            return;
        }

        int count = plan.Path.Count;
        Summary = FormattableString.Invariant(
            $"Indoor path: {plan.LengthMeters:0.#} m, {count} waypoints"
        );
        var lines = plan.Path.Take(MaximumDisplayedWaypoints)
            .Select((point, index) =>
            {
                var local = PluginDungeonFloorplan.ToLandblockLocal(point);
                return FormattableString.Invariant(
                    $"{index + 1}. Cell 0x{point.CellId:X8}: E {local.X:0.#}, N {local.Y:0.#}, Z {local.Z:0.#} m"
                );
            })
            .ToList();
        if (count > MaximumDisplayedWaypoints)
            lines.Add($"... {count - MaximumDisplayedWaypoints} more waypoints");
        Waypoints = lines;
    }
}
