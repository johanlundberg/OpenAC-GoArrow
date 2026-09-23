using System.Net;
using System.Net.Http;
using AcDream.Plugin.Abstractions;
using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public class WarcryAtlasDataProviderTests
{
    [Fact]
    public void ValidateAtlasXml_RequiresLocationRecords()
    {
        Assert.Throws<InvalidDataException>(() =>
            WarcryAtlasDataProvider.ValidateAtlasXml("<atlas />")
        );
    }

    [Fact]
    public void ValidateAtlasXml_ReturnsLocationCount()
    {
        const string xml = "<atlas><location /><location /></atlas>";
        Assert.Equal(2, WarcryAtlasDataProvider.ValidateAtlasXml(xml));
    }

    [Fact]
    public async Task DownloadAsync_ValidatesAndCachesData()
    {
        const string xml =
            "<atlas><location><id>1</id><name>Test</name>"
            + "<latitude>1</latitude><longitude>2</longitude></location></atlas>";
        var storage = new MemoryStorage();
        using var client = new HttpClient(new StubHandler(xml));
        var provider = new WarcryAtlasDataProvider(
            storage,
            client,
            "https://example.test/atlas.xml"
        );

        string downloaded = await provider.DownloadAsync();

        Assert.Equal(xml, downloaded);
        Assert.Equal(1, provider.LastDownloadedLocationCount);
        Assert.Equal(xml, storage.ReadText("data/warcry-atlas.xml"));
        Assert.Equal(xml, provider.ReadCached());
    }

    [Fact]
    public async Task DownloadAsync_RejectsHttpFailure()
    {
        using var client = new HttpClient(new StubHandler("", HttpStatusCode.NotFound));
        var provider = new WarcryAtlasDataProvider(
            new MemoryStorage(),
            client,
            "https://example.test/atlas.xml"
        );

        await Assert.ThrowsAsync<HttpRequestException>(() => provider.DownloadAsync());
    }

    [Fact]
    public async Task DownloadAsync_RequiresConfiguredUrl()
    {
        var provider = new WarcryAtlasDataProvider(new MemoryStorage());

        await Assert.ThrowsAsync<InvalidOperationException>(() => provider.DownloadAsync());
    }

    private sealed class StubHandler : HttpMessageHandler
    {
        private readonly string _content;
        private readonly HttpStatusCode _status;

        public StubHandler(string content, HttpStatusCode status = HttpStatusCode.OK)
        {
            _content = content;
            _status = status;
        }

        protected override Task<HttpResponseMessage> SendAsync(
            HttpRequestMessage request,
            CancellationToken cancellationToken
        )
        {
            return Task.FromResult(
                new HttpResponseMessage(_status)
                {
                    Content = new StringContent(_content),
                    RequestMessage = request,
                }
            );
        }
    }

    private sealed class MemoryStorage : IPluginStorage
    {
        private readonly Dictionary<string, string> _values = new();
        public bool IsAvailable => true;

        public string? ReadText(string key) =>
            _values.TryGetValue(key, out var value) ? value : null;

        public void WriteText(string key, string content) => _values[key] = content;
    }
}
