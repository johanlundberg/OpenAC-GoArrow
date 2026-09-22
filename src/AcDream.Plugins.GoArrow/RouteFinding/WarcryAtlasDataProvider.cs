using System.Net;
using System.Net.Http;
using System.Xml;
using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow.RouteFinding;

/// <summary>
/// Downloads and validates the Crossroads of Dereth/Warcry Atlas location
/// database. Downloading is explicit; the provider never runs during plugin
/// startup.
/// </summary>
internal sealed class WarcryAtlasDataProvider
{
    private const string CacheKey = "data/warcry-atlas.xml";
    private const string MetadataKey = "data/warcry-atlas.metadata.json";
    private const int MaximumDownloadBytes = 16 * 1024 * 1024;

    private readonly HttpClient _httpClient;
    private readonly IPluginStorage _storage;
    private readonly string _url;

    public WarcryAtlasDataProvider(
        IPluginStorage storage,
        HttpClient? httpClient = null,
        string? url = null)
    {
        _storage = storage;
        _httpClient = httpClient ?? new HttpClient();
        _url = url ?? string.Empty;
    }

    public string Url => _url;

    /// <summary>
    /// Downloads the Atlas XML, validates its root and location records, and
    /// stores a complete cached copy only after validation succeeds.
    /// </summary>
    public async Task<string> DownloadAsync(CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(_url))
            throw new InvalidOperationException("Set a location data URL before downloading.");
        using HttpResponseMessage response = await _httpClient.GetAsync(
            _url,
            HttpCompletionOption.ResponseHeadersRead,
            cancellationToken).ConfigureAwait(false);

        if (response.StatusCode != HttpStatusCode.OK)
        {
            throw new HttpRequestException(
                $"Atlas download returned HTTP {(int)response.StatusCode} ({response.ReasonPhrase}).");
        }

        byte[] payload = await response.Content.ReadAsByteArrayAsync(cancellationToken)
            .ConfigureAwait(false);
        if (payload.Length == 0)
            throw new InvalidDataException("Atlas download was empty.");
        if (payload.Length > MaximumDownloadBytes)
        {
            throw new InvalidDataException(
                $"Atlas download is larger than the {MaximumDownloadBytes / (1024 * 1024)} MiB limit.");
        }

        string xml = System.Text.Encoding.UTF8.GetString(payload);
        int locationCount = ValidateAtlasXml(xml);
        WriteCache(xml);
        LastDownloadedLocationCount = locationCount;
        return xml;
    }

    /// <summary>
    /// Returns the last validated cache, or null when no cache is available.
    /// </summary>
    public string? ReadCached(TimeSpan? maxAge = null)
    {
        if (maxAge is { } age && age > TimeSpan.Zero)
        {
            CacheMetadata? metadata = _storage.ReadJson<CacheMetadata>(MetadataKey);
            if (metadata is null || DateTimeOffset.UtcNow - metadata.DownloadedAt > age)
                return null;
        }
        string? cached = _storage.ReadText(CacheKey);
        if (string.IsNullOrWhiteSpace(cached))
            return null;

        ValidateAtlasXml(cached);
        return cached;
    }

    public int LastDownloadedLocationCount { get; private set; }

    /// <summary>
    /// Validates the expected Atlas root and returns the number of records.
    /// </summary>
    public static int ValidateAtlasXml(string xml)
    {
        var settings = new XmlReaderSettings
        {
            DtdProcessing = DtdProcessing.Prohibit,
            XmlResolver = null,
            MaxCharactersInDocument = MaximumDownloadBytes,
        };
        using var stringReader = new StringReader(xml);
        using XmlReader reader = XmlReader.Create(stringReader, settings);
        var document = new XmlDocument { XmlResolver = null };
        document.Load(reader);

        if (!string.Equals(document.DocumentElement?.Name, "atlas", StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException("Atlas XML root element must be <atlas>.");

        int count = document.SelectNodes("/atlas/location")?.Count ?? 0;
        if (count == 0)
            throw new InvalidDataException("Atlas XML contains no location records.");
        return count;
    }

    private void WriteCache(string xml)
    {
        if (!_storage.IsAvailable)
            return;

        _storage.WriteText(CacheKey, xml);
        _storage.WriteJson(MetadataKey, new CacheMetadata(DateTimeOffset.UtcNow, ValidateAtlasXml(xml), _url));
    }

    private sealed record CacheMetadata(DateTimeOffset DownloadedAt, int LocationCount, string SourceUrl);
}
