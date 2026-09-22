using System.IO.Compression;

namespace AcDream.Plugins.GoArrow.Tests;

internal static class DungeonMapTestData
{
    private static readonly byte[] Gif = Convert.FromBase64String("R0lGODlhAQABAAD/ACwAAAAAAQABAAACAUwAOw==");
    private static readonly byte[] Png = Convert.FromBase64String(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/lXcAAAAASUVORK5CYII=");

    public static DungeonMapCatalog CreateCatalog()
    {
        byte[] archive = CreateArchive();
        return new DungeonMapCatalog(() => new MemoryStream(archive, writable: false));
    }

    public static byte[] CreateArchive()
    {
        using var buffer = new MemoryStream();
        using (var archive = new ZipArchive(buffer, ZipArchiveMode.Create, leaveOpen: true))
        {
            Write(archive, "Dungeon Map Cache/dungeons.txt",
                System.Text.Encoding.UTF8.GetBytes("0001;Remote Empyrean Vault;\n0002;Viamontian Garrison;\n00D1;Tall Map;\n"));
            Write(archive, "Dungeon Map Cache/0001.gif", Gif);
            Write(archive, "Dungeon Map Cache/0002.gif", Gif);
            Write(archive, "Dungeon Map Cache/00D1.png", Png);
        }
        return buffer.ToArray();
    }

    public static void WriteExtracted(string directory)
    {
        string nested = Path.Combine(directory, "Dungeon Map Cache");
        Directory.CreateDirectory(nested);
        File.WriteAllBytes(Path.Combine(nested, "0001.gif"), Gif);
        File.WriteAllText(Path.Combine(nested, "dungeons.txt"), "0001;Remote Empyrean Vault;\n");
    }

    private static void Write(ZipArchive archive, string name, byte[] bytes)
    {
        using var stream = archive.CreateEntry(name).Open();
        stream.Write(bytes);
    }
}
