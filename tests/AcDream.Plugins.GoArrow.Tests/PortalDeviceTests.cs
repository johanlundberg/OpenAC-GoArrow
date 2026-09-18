using AcDream.Plugins.GoArrow.RouteFinding;

namespace AcDream.Plugins.GoArrow.Tests;

public class PortalDeviceTests
{
    [Fact]
    public void PortalDevice_Create_StoresValues()
    {
        var pd = new PortalDevice("Holtburg", "Holtburg South", "Dereth");
        Assert.Equal("Holtburg", pd.Destination);
        Assert.Equal("Holtburg South", pd.Via);
        Assert.Equal("Dereth", pd.Landmass);
    }

    [Fact]
    public void PortalDevice_FromCsvLine_ParsesCorrectly()
    {
        var pd = PortalDevice.FromCsvLine("Holtburg;Holtburg South Portal;Dereth");
        Assert.Equal("Holtburg", pd.Destination);
        Assert.Equal("Holtburg South Portal", pd.Via);
        Assert.Equal("Dereth", pd.Landmass);
    }

    [Fact]
    public void PortalDevice_FromCsvLine_NoLandmass()
    {
        var pd = PortalDevice.FromCsvLine("Holtburg;Portal Device;");
        Assert.Equal("Holtburg", pd.Destination);
        Assert.Equal("Portal Device", pd.Via);
        Assert.Empty(pd.Landmass);
    }

    [Fact]
    public void PortalDevice_Equality_SameDestinationAndVia()
    {
        var a = new PortalDevice("Holtburg", "Portal", "Dereth");
        var b = new PortalDevice("Holtburg", "Portal", "Dereth");
        Assert.Equal(a, b);
    }

    [Fact]
    public void PortalDevice_Equality_CaseInsensitive()
    {
        var a = new PortalDevice("Holtburg", "Portal", "Dereth");
        var b = new PortalDevice("holtburg", "Portal", "Dereth");
        Assert.Equal(a, b);
    }

    [Fact]
    public void PortalDevice_Equality_DifferentDestination()
    {
        var a = new PortalDevice("Holtburg", "Portal", "Dereth");
        var b = new PortalDevice("Shoushi", "Portal", "Dereth");
        Assert.NotEqual(a, b);
    }

    [Fact]
    public void PortalDevice_ToString_FormatsCorrectly()
    {
        var pd = new PortalDevice("Test", "Via", "Land");
        Assert.Equal("Test;Via;Land", pd.ToString());
    }
}