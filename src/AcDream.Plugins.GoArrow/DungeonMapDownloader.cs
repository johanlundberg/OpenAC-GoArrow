using System.Net;
using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow;

/// <summary>Downloads a user-requested dungeon map archive into plugin storage.</summary>
internal sealed class DungeonMapDownloader
{
    private const long MaximumDownloadBytes = 64L * 1024 * 1024;
    private static readonly HttpClient SharedHttpClient = new();
    private readonly IPluginStorage _storage;
    private readonly HttpClient _httpClient;

    public DungeonMapDownloader(IPluginStorage storage, HttpClient? httpClient = null)
    {
        _storage = storage;
        _httpClient = httpClient ?? SharedHttpClient;
    }

    public async Task<int> DownloadAsync(string url, CancellationToken cancellationToken = default)
    {
        string directory =
            DungeonMapCatalog.UserMapDirectory(_storage)
            ?? throw new InvalidOperationException("Plugin storage is unavailable.");
        using HttpResponseMessage response = await _httpClient
            .GetAsync(url, HttpCompletionOption.ResponseHeadersRead, cancellationToken)
            .ConfigureAwait(false);
        if (response.StatusCode != HttpStatusCode.OK)
            throw new HttpRequestException(
                $"Dungeon map download returned HTTP {(int)response.StatusCode}."
            );
        if (response.Content.Headers.ContentLength > MaximumDownloadBytes)
            throw new InvalidDataException("Dungeon map archive exceeds the 64 MiB limit.");

        string temporary = Path.Combine(directory, $"Dungeon_Map_Cache.{Guid.NewGuid():N}.tmp");
        try
        {
            await using (
                var output = new FileStream(
                    temporary,
                    FileMode.CreateNew,
                    FileAccess.Write,
                    FileShare.None,
                    81920,
                    useAsync: true
                )
            )
            await using (
                Stream input = await response
                    .Content.ReadAsStreamAsync(cancellationToken)
                    .ConfigureAwait(false)
            )
            {
                var buffer = new byte[81920];
                long total = 0;
                int count;
                while (
                    (count = await input.ReadAsync(buffer, cancellationToken).ConfigureAwait(false))
                    > 0
                )
                {
                    total += count;
                    if (total > MaximumDownloadBytes)
                        throw new InvalidDataException(
                            "Dungeon map archive exceeds the 64 MiB limit."
                        );
                    await output
                        .WriteAsync(buffer.AsMemory(0, count), cancellationToken)
                        .ConfigureAwait(false);
                }
            }

            var catalog = new DungeonMapCatalog(() => File.OpenRead(temporary));
            if (catalog.Count == 0)
                throw new InvalidDataException("Dungeon map archive contains no usable maps.");
            File.Move(temporary, Path.Combine(directory, "Dungeon_Map_Cache.zip"), overwrite: true);
            return catalog.Count;
        }
        finally
        {
            if (File.Exists(temporary))
                File.Delete(temporary);
        }
    }
}
