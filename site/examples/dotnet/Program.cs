// The tour: every query of the guide, run on a released pack.
//
// Usage: dotnet run -c Release -- world-np-lite.tgd

// #region import
using System.Globalization;
using Teispace.TeiGeodata;
// #endregion import

string pack = args[0];
// Numbers print with a decimal point whatever the user's culture.
CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;

Console.WriteLine("=== open");
// #region open
using var engine = new Engine();
engine.OpenFile(pack);
Console.WriteLine($"layers: {engine.Layers()}");
// #endregion open

Console.WriteLine("=== search");
// #region search
foreach (var p in new BlobReader(engine.Search("Pokhara", 3)))
{
    Console.WriteLine($"{p["id"]}  {p["name"]}  {p["kind"]}  {p["lat"]:F4}, {p["lon"]:F4}  score {p["score"]}");
}
// #endregion search

Console.WriteLine("=== search-parent");
// #region search-parent
foreach (var p in new BlobReader(engine.Search("Kathmandu, Bagmati", 2)))
{
    Console.WriteLine($"{p["id"]}  {p["name"]}  {p["kind"]} {p["level"]}");
}
// #endregion search-parent

Console.WriteLine("=== search-focus");
// #region search-focus
// Many places share a name; a focus (the user's location) ranks the near ones first.
var options = new SearchOptions { FocusLat = 28.7, FocusLon = 80.6, FocusScaleM = 30_000 };
foreach (var p in new BlobReader(engine.SearchWith("Shivapur", 3, options)))
{
    Console.WriteLine($"{p["id"]}  {p["name"]}  {p["lat"]:F4}, {p["lon"]:F4}");
}
// #endregion search-focus

Console.WriteLine("=== get");
// #region get
var place = new BlobReader(engine.Get("tgd:np/1xhb")).Single();
Console.WriteLine($"{place["name"]} {place["kind"]}");
Console.WriteLine($"at {place["lat"]:F6}, {place["lon"]:F6}, {place["elevation"]} m");
Console.WriteLine($"timezone {place["tz"]} parent {place["parent"]}");
// #endregion get

Console.WriteLine("=== hierarchy");
// #region hierarchy
foreach (var p in new BlobReader(engine.Ancestors("tgd:np/1xhb")))
{
    Console.WriteLine($"{p["kind"]} {p["level"]}  {p["name"]}  {p["iso"]}".TrimEnd());
}
var children = new BlobReader(engine.Children("tgd:np/295c"));
Console.WriteLine($"Kaski has {children.Rows} children");
// #endregion hierarchy

Console.WriteLine("=== reverse");
// #region reverse
foreach (var p in new BlobReader(engine.Reverse(27.7172, 85.3240, 3)))
{
    Console.WriteLine($"{p["name"]}  {p["meters"]:F0} m at {p["bearing"]:F0}°");
}
// #endregion reverse

Console.WriteLine("=== radius");
// #region radius
var within = new BlobReader(engine.WithinRadius(27.7172, 85.3240, 500, 10)).Records();
Console.WriteLine($"{within.Count} places within 500 m; farthest {within[^1]["meters"]:F0} m");
// #endregion radius

Console.WriteLine("=== nearby");
// #region nearby
foreach (var p in new BlobReader(engine.Nearby("tgd:np/2cn", 3)))
{
    Console.WriteLine($"{p["name"]}  {p["meters"]:F0} m");
}
// #endregion nearby

Console.WriteLine("=== admin");
// #region admin
foreach (var p in new BlobReader(engine.AdminAt(28.2096, 83.9856)))
{
    Console.WriteLine($"{p["level"]}  {p["name"]}  {p["iso"]}".TrimEnd());
}
var state = new BlobReader(engine.ByIso("NP-P4")).Single();
Console.WriteLine($"NP-P4 is {state["name"]} {state["id"]}");
// #endregion admin

Console.WriteLine("=== timezone");
// #region timezone
var zone = (string)new BlobReader(engine.TzAt(27.7172, 85.3240)).Single()["tz"]!;
var offset = new BlobReader(engine.TzOffsetAt(zone, 1_700_000_000)).Single();
Console.WriteLine($"{zone} {offset["seconds"]} s east of UTC, {offset["abbr"]} dst {offset["dst"]}");
var local = new BlobReader(engine.TzLocal("America/New_York", 2026, 3, 8, 2, 30, 0)).Single();
Console.WriteLine($"02:30 on 2026-03-08 in New York: {local["kind"]} {local["before"]} -> {local["after"]}");
Console.WriteLine($"tzdb {engine.TzVersion()}");
// #endregion timezone

Console.WriteLine("=== elevation");
// #region elevation
var height = new BlobReader(engine.ElevationAt(27.7172, 85.3240)).Single();
Console.WriteLine($"{height["meters"]:F1} m above {height["datum"]} ({height["source"]}, {height["resolution_m"]:F0} m grid)");
// #endregion elevation

Console.WriteLine("=== distance");
// #region distance
double meters = Geodata.DistanceM(27.7172, 85.3240, 28.2096, 83.9856);
double heading = Geodata.BearingDeg(27.7172, 85.3240, 28.2096, 83.9856);
Console.WriteLine($"Kathmandu to Pokhara: {meters / 1000:F1} km, heading {heading:F0}°");
var apart = new BlobReader(engine.Distance("tgd:np/2cn", "tgd:np/1xhb")).Single();
Console.WriteLine($"place to place: {(double)apart["meters"]! / 1000:F1} km at {apart["bearing"]:F0}°");
// #endregion distance

Console.WriteLine("=== boundary");
// #region boundary
var shape = new BlobReader(engine.Boundary("tgd:np/2gm", 0)).Single();
string geojson = (string)shape["geojson"]!; // a GeoJSON Feature, for any map library
using var feature = System.Text.Json.JsonDocument.Parse(geojson);
string type = feature.RootElement.GetProperty("geometry").GetProperty("type").GetString()!;
Console.WriteLine($"{type} of {shape["vertices"]} vertices at level {shape["lod"]}");
Console.WriteLine($"tolerance {shape["tolerance_m"]:F0} m, {geojson.Length} bytes of GeoJSON");
// #endregion boundary

Console.WriteLine("=== errors");
// #region errors
try
{
    engine.Reverse(91, 0, 1);
}
catch (TeiGeodataException error) when (error.Code == ErrorCode.InvalidArgument)
{
    Console.WriteLine($"invalid argument: {error.Message}");
}
// #endregion errors
