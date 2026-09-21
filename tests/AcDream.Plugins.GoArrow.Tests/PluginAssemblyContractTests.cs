using System.Reflection;
using AcDream.Plugin.Abstractions;

namespace AcDream.Plugins.GoArrow.Tests;

/// <summary>
/// Guard-rails the exact loader behavior that failed when GoArrow was built
/// against a newer OpenAC abstractions assembly than the host provided: the host
/// calls GetTypes() on the entry assembly and hunts for a public IAcDreamPlugin.
/// If a referenced contract type is missing at load time, GetTypes() throws a
/// ReflectionTypeLoadException, the scan finds no implementation, and the host
/// reports "No IAcDreamPlugin implementation found".
/// </summary>
public sealed class PluginAssemblyContractTests
{
    [Fact]
    public void EntryAssemblyResolvesExactlyOnePublicIAcDreamPlugin()
    {
        var assembly = typeof(GoArrowPlugin).Assembly;

        Type[] types = assembly.GetTypes(); // throws if a referenced type is missing

        Type[] candidates = types
            .Where(static type => !type.IsAbstract && !type.IsInterface)
            .Where(static type => typeof(IAcDreamPlugin).IsAssignableFrom(type))
            .Where(static type => type.IsPublic)
            .ToArray();

        Assert.Single(candidates);
        Assert.Equal("AcDream.Plugins.GoArrow.GoArrowPlugin", candidates[0].FullName);
    }

    [Fact]
    public void EntryAssemblyReferencesAbstractionsOnly()
    {
        var assembly = typeof(GoArrowPlugin).Assembly;

        // OpenAC must supply the contract; no extra AcDream.AcDream runtime copy may ride along.
        var acDreamReferences = assembly
            .GetReferencedAssemblies()
            .Where(static name => name.Name is not null && name.Name.StartsWith("AcDream.", StringComparison.Ordinal))
            .Select(static name => name.Name!)
            .ToArray();

        Assert.Equal(["AcDream.Plugin.Abstractions"], acDreamReferences);
    }
}