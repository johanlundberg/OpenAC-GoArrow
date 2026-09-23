using System.Globalization;
using System.IO.Compression;
using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow;

/// <summary>Indexes dungeon images supplied by the player, either loose or zipped.</summary>
internal sealed class DungeonMapCatalog
{
    public const string StorageDirectory = "dungeon-maps";
    private readonly Func<Stream>? _openArchive;
    private readonly string? _directory;
    private readonly Dictionary<int, DungeonMapEntry> _maps = new();

    public static DungeonMapCatalog? OpenUserMaps(IPluginStorage storage)
    {
        string? directory = UserMapDirectory(storage);
        if (directory is null)
            return null;

        var loose = new DungeonMapCatalog(directory);
        if (loose.Count > 0)
            return loose;

        string archive = Path.Combine(directory, "Dungeon_Map_Cache.zip");
        if (!File.Exists(archive))
            return null;

        var zipped = new DungeonMapCatalog(() => File.OpenRead(archive));
        return zipped.Count > 0 ? zipped : null;
    }

    public static string? UserMapDirectory(IPluginStorage storage)
    {
        if (!storage.IsAvailable || storage.RootPath is not { } storageRoot)
            return null;
        storage.EnsureDirectory(StorageDirectory);
        return Path.Combine(storageRoot, StorageDirectory);
    }

    /// <summary>Reads a ZIP source. The stream is reopened when an image is requested.</summary>
    public DungeonMapCatalog(Func<Stream> openArchive)
    {
        _openArchive = openArchive;
        using var source = openArchive();
        using var archive = new ZipArchive(source, ZipArchiveMode.Read);
        var index = archive.Entries.FirstOrDefault(entry =>
            Path.GetFileName(entry.FullName)
                .Equals("dungeons.txt", StringComparison.OrdinalIgnoreCase)
        );
        var names = index is null ? new Dictionary<int, string>() : ReadNames(index.Open());

        foreach (ZipArchiveEntry entry in archive.Entries)
            AddImage(entry.FullName, names);
    }

    /// <summary>Reads an extracted cache, including the original nested folder layout.</summary>
    public DungeonMapCatalog(string directory)
    {
        _directory = directory;
        if (!Directory.Exists(directory))
            return;

        var files = Directory.EnumerateFiles(directory, "*", SearchOption.AllDirectories).ToArray();
        string? index = files.FirstOrDefault(file =>
            Path.GetFileName(file).Equals("dungeons.txt", StringComparison.OrdinalIgnoreCase)
        );
        var names = index is null ? new Dictionary<int, string>() : ReadNames(File.OpenRead(index));
        foreach (string file in files)
            AddImage(Path.GetRelativePath(directory, file), names);
    }

    public int Count => _maps.Count;

    public bool TryGet(int dungeonId, out DungeonMapEntry entry) =>
        _maps.TryGetValue(dungeonId, out entry!);

    public Stream OpenImage(DungeonMapEntry map)
    {
        if (_directory is not null)
            return File.OpenRead(Path.Combine(_directory, map.RelativePath));

        using var sourceArchive = _openArchive!();
        using var archive = new ZipArchive(sourceArchive, ZipArchiveMode.Read);
        ZipArchiveEntry entry =
            archive.GetEntry(map.RelativePath)
            ?? throw new FileNotFoundException(
                $"Dungeon map {map.Id:X4} is missing from the archive."
            );
        using var source = entry.Open();
        var copy = new MemoryStream((int)entry.Length);
        source.CopyTo(copy);
        copy.Position = 0;
        return copy;
    }

    private void AddImage(string relativePath, Dictionary<int, string> names)
    {
        string filename = Path.GetFileName(relativePath);
        string extension = Path.GetExtension(filename);
        if (
            !extension.Equals(".gif", StringComparison.OrdinalIgnoreCase)
            && !extension.Equals(".png", StringComparison.OrdinalIgnoreCase)
        )
            return;
        if (!TryParseId(Path.GetFileNameWithoutExtension(filename), out int id))
            return;

        var candidate = new DungeonMapEntry(
            id,
            names.TryGetValue(id, out string? name) && name.Length > 0 ? name : $"Dungeon {id:X4}",
            relativePath
        );
        if (
            !_maps.TryGetValue(id, out var existing)
            || extension.Equals(".png", StringComparison.OrdinalIgnoreCase)
                && !existing.RelativePath.EndsWith(".png", StringComparison.OrdinalIgnoreCase)
        )
            _maps[id] = candidate;
    }

    private static Dictionary<int, string> ReadNames(Stream source)
    {
        using (source)
        using (var reader = new StreamReader(source))
        {
            var names = new Dictionary<int, string>();
            while (reader.ReadLine() is { } line)
            {
                string[] fields = line.TrimStart('\uFEFF').Split(';', 3);
                if (fields.Length >= 2 && TryParseId(fields[0], out int id))
                    names.TryAdd(id, fields[1].Trim());
            }
            return names;
        }
    }

    private static bool TryParseId(string value, out int id)
    {
        id = 0;
        return value.Length == 4
            && int.TryParse(value, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out id);
    }
}

internal sealed record DungeonMapEntry(int Id, string Name, string RelativePath);
