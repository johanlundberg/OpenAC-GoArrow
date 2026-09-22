using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow.Tests;

public sealed class DungeonMapCatalogTests
{
    [Fact]
    public void EmptyUserMapFolderIsCreatedWithoutLoadingMaps()
    {
        string root = Path.Combine(Path.GetTempPath(), $"goarrow-maps-{Guid.NewGuid():N}");
        try
        {
            Assert.Null(DungeonMapCatalog.OpenUserMaps(new DirectoryStorage(root)));
            Assert.True(Directory.Exists(Path.Combine(root, DungeonMapCatalog.StorageDirectory)));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void UserArchiveIndexesDungeonIdsAndOpensImages()
    {
        var catalog = DungeonMapTestData.CreateCatalog();

        Assert.Equal(3, catalog.Count);
        Assert.True(catalog.TryGet(0x0001, out var vault));
        Assert.Equal("Remote Empyrean Vault", vault.Name);
        using var image = catalog.OpenImage(vault);
        Span<byte> header = stackalloc byte[3];
        Assert.Equal(3, image.Read(header));
        Assert.Equal("GIF", System.Text.Encoding.ASCII.GetString(header));
        Assert.True(catalog.TryGet(0x00D1, out var tallMap));
        Assert.EndsWith(".png", tallMap.RelativePath);
        Assert.False(catalog.TryGet(0xFFFF, out _));
    }

    [Fact]
    public void UserMapFolderAcceptsExtractedCacheAndPrefersLooseImages()
    {
        string root = Path.Combine(Path.GetTempPath(), $"goarrow-maps-{Guid.NewGuid():N}");
        string directory = Path.Combine(root, "dungeon-maps");
        Directory.CreateDirectory(directory);
        try
        {
            DungeonMapTestData.WriteExtracted(directory);
            File.WriteAllBytes(Path.Combine(directory, "Dungeon_Map_Cache.zip"), DungeonMapTestData.CreateArchive());
            var catalog = DungeonMapCatalog.OpenUserMaps(new DirectoryStorage(root));

            Assert.NotNull(catalog);
            Assert.Equal(1, catalog!.Count);
            Assert.True(catalog.TryGet(0x0001, out var vault));
            Assert.Equal("Remote Empyrean Vault", vault.Name);
            using var image = catalog.OpenImage(vault);
            Assert.True(image.Length > 10);
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
    }

    [Fact]
    public void UserMapFolderAcceptsZipWithoutExtraction()
    {
        string root = Path.Combine(Path.GetTempPath(), $"goarrow-maps-{Guid.NewGuid():N}");
        string directory = Path.Combine(root, "dungeon-maps");
        Directory.CreateDirectory(directory);
        try
        {
            File.WriteAllBytes(Path.Combine(directory, "Dungeon_Map_Cache.zip"), DungeonMapTestData.CreateArchive());
            var catalog = DungeonMapCatalog.OpenUserMaps(new DirectoryStorage(root));

            Assert.NotNull(catalog);
            Assert.Equal(3, catalog!.Count);
            Assert.True(catalog.TryGet(0x0002, out _));
        }
        finally
        {
            Directory.Delete(root, recursive: true);
        }
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
