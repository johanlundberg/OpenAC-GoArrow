using System.Net;
using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class DungeonMapDownloaderTests
{
    [Fact]
    public async Task ValidDownloadInstallsMapArchive()
    {
        string root = Path.Combine(Path.GetTempPath(), $"goarrow-download-{Guid.NewGuid():N}");
        try
        {
            using var client = new HttpClient(new StubHandler(DungeonMapTestData.CreateArchive()));
            var storage = new DirectoryStorage(root);
            int count = await new DungeonMapDownloader(storage, client).DownloadAsync(
                "https://example.test/maps.zip"
            );

            Assert.Equal(3, count);
            Assert.Equal(3, DungeonMapCatalog.OpenUserMaps(storage)!.Count);
            Assert.Single(Directory.GetFiles(Path.Combine(root, "dungeon-maps")));
        }
        finally
        {
            if (Directory.Exists(root))
                Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public async Task InvalidDownloadKeepsPreviousArchive()
    {
        string root = Path.Combine(Path.GetTempPath(), $"goarrow-download-{Guid.NewGuid():N}");
        try
        {
            var storage = new DirectoryStorage(root);
            string directory = DungeonMapCatalog.UserMapDirectory(storage)!;
            byte[] previous = DungeonMapTestData.CreateArchive();
            File.WriteAllBytes(Path.Combine(directory, "Dungeon_Map_Cache.zip"), previous);
            using var client = new HttpClient(new StubHandler("not a ZIP"u8.ToArray()));

            await Assert.ThrowsAnyAsync<Exception>(() =>
                new DungeonMapDownloader(storage, client).DownloadAsync(
                    "https://example.test/maps.zip"
                )
            );

            Assert.Equal(
                previous,
                File.ReadAllBytes(Path.Combine(directory, "Dungeon_Map_Cache.zip"))
            );
            Assert.Single(Directory.GetFiles(directory));
        }
        finally
        {
            if (Directory.Exists(root))
                Directory.Delete(root, recursive: true);
        }
    }

    private sealed class StubHandler(byte[] payload) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(
            HttpRequestMessage request,
            CancellationToken cancellationToken
        ) =>
            Task.FromResult(
                new HttpResponseMessage(HttpStatusCode.OK)
                {
                    Content = new ByteArrayContent(payload),
                }
            );
    }

    private sealed class DirectoryStorage(string root) : IPluginStorage
    {
        public bool IsAvailable => true;
        public string? RootPath => root;

        public bool EnsureDirectory(string prefix)
        {
            Directory.CreateDirectory(Path.Combine(root, prefix));
            return true;
        }
    }
}
