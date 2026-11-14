// Parity: the .NET binding gives the results bindings/parity.json expects (checked there against
// Rust). A console program, not a test project, so that the same checks run under the JIT and
// as a NativeAOT executable, with no test framework to download:
//
//     TEIGEODATA_LIB=../../target/release/libteigeodata.dylib dotnet run -c Release
//
// It prints one line per check and exits 1 if any failed.

using System.Diagnostics;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using Teispace.TeiGeodata;

string bindings = FindBindings();
using JsonDocument document = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(bindings, "parity.json")));
JsonElement parity = document.RootElement;
string Fixture(string rel) => Path.Combine(bindings, rel);

var checks = new (string Name, Action Run)[]
{
    ("version", CheckVersion),
    ("error_codes", CheckErrorCodes),
    ("fold", CheckFold),
    ("sample_rows", CheckSampleRows),
    ("distance", CheckDistance),
    ("progress", CheckProgress),
    ("engine", CheckEngineQueries),
    ("open_options", CheckOpenOptions),
    ("disposed", CheckDisposed),
    ("concurrent", CheckConcurrent),
    ("tz", CheckTz),
    ("store", CheckStore),
    ("job_cancel", CheckJobCancel),
    ("reader_fuzz", CheckReaderFuzz),
};
int failed = 0;
foreach (var (name, run) in checks)
{
    try
    {
        run();
        Console.WriteLine($"--- PASS: {name}");
    }
    catch (SkipException e)
    {
        Console.WriteLine($"--- SKIP: {name} ({e.Message})");
    }
    catch (Exception e)
    {
        failed++;
        Console.WriteLine($"--- FAIL: {name}\n{e}");
    }
}
Console.WriteLine(failed == 0 ? "ok" : $"FAIL: {failed} of {checks.Length}");
return failed == 0 ? 0 : 1;

// ---- the checks ----

void CheckVersion()
{
    Equal(parity.Str("version"), Geodata.Version(), "version");
    Equal(Library.Version, Geodata.Version(), "Library.Version");
    Equal(parity.Str("components"), Geodata.Components(), "components");
    Geodata.Noop();
    string loaded = Library.LoadedPath ?? throw new Exception("no loaded path");
    Assert(loaded.Contains("teigeodata"), $"loaded path {loaded}");
    Library.Load(loaded);
    Throws<InvalidOperationException>(() => Library.Load("/elsewhere/libteigeodata.so"));
}

void CheckErrorCodes()
{
    foreach (JsonProperty code in parity.GetProperty("error_codes").EnumerateObject())
    {
        Equal(code.Name, ((ErrorCode)code.Value.GetInt32()).ToString(), "code name");
    }
}

void CheckFold()
{
    foreach (JsonElement pair in parity.GetProperty("fold").EnumerateArray())
    {
        Equal(pair[1].GetString(), Geodata.Fold(pair[0].GetString()!), "fold");
    }
    // A lone surrogate is not text: refused before the call, never replaced.
    Throws<ArgumentException>(() => Geodata.Fold("\uD800"));
    string longText = new('a', 10_000);
    Equal(longText, Geodata.Fold(longText), "a string past the stack buffer");
}

void CheckSampleRows()
{
    JsonElement s = parity.GetProperty("sample_rows");
    byte[] blob = Geodata.SampleRows(s.U32("rows"));
    Equal(s.GetProperty("bytes").GetInt32(), blob.Length, "bytes");
    var r = new BlobReader(blob);
    Equal(string.Join(",", s.Strings("columns")), string.Join(",", r.Columns), "columns");
    double lat = 0;
    foreach (double v in r.Values<double>("lat"))
    {
        lat += v;
    }
    double elevation = Numbers(r, "elevation").Sum();
    Equal(s.GetProperty("lat_sum").GetDouble(), lat, "lat sum");
    Equal(s.GetProperty("elevation_sum").GetDouble(), elevation, "elevation sum");
    Equal(s.Str("last_name"), r.Strings("name")[^1], "last name");
    Throws<InvalidCastException>(() => r.Values<float>("lat"));
    Throws<KeyNotFoundException>(() => r.Strings("nothing"));
    Equal(parity.U32("sample_rows_max"), Geodata.SampleRowsMax, "SampleRowsMax");
    var e = Code(ErrorCode.InvalidArgument, () => Geodata.SampleRows(Geodata.SampleRowsMax + 1));
    Assert(e.Message.Contains("at most"), e.Message);
}

void CheckDistance()
{
    JsonElement d = parity.GetProperty("distance_m");
    double[] a = d.Doubles("args");
    double got = Geodata.DistanceM(a[0], a[1], a[2], a[3]);
    Assert(Math.Abs(got - d.GetProperty("metres").GetDouble()) <= 1e-6, $"distance {got}");
    double[] o = d.Doubles("out_of_range");
    Code(ErrorCode.InvalidArgument, () => Geodata.DistanceM(o[0], o[1], o[2], o[3]));
}

void CheckProgress()
{
    var seen = new List<uint>();
    Equal(3u, Geodata.Progress(3, s => { seen.Add(s); return true; }), "steps");
    Equal("1,2,3", string.Join(",", seen), "seen");
    JsonElement g = parity.GetProperty("progress");
    uint after = g.U32("cancel_after");
    var e = Code(ErrorCode.Cancelled, () => Geodata.Progress(g.U32("steps"), s => s < after));
    Assert(e.Message.Contains(g.Str("cancelled_message")), e.Message);
    Code(ErrorCode.InvalidArgument, () => Geodata.Progress(1, null));
    // An exception in a callback reaches the caller, with its stack, and leaves the library sound.
    var boom = new InvalidTimeZoneException("boom");
    var thrown = Throws<InvalidTimeZoneException>(() => Geodata.Progress(5, _ => throw boom));
    Assert(ReferenceEquals(thrown, boom), "another exception came back");
    Equal(2u, Geodata.Progress(2, _ => true), "steps after an exception");
}

void CheckEngineQueries()
{
    JsonElement p = parity.GetProperty("engine");
    using var engine = new Engine();
    Equal(0u, engine.OpenFile(Fixture(p.Str("fixture"))), "layer");
    JsonElement get = p.GetProperty("get");
    var got = new BlobReader(engine.Get(get.Str("id")));
    Equal(get.Str("name"), got.Strings("name").Single(), "name");
    Equal(get.Str("kind"), got.Strings("kind").Single(), "kind");
    Equal(get.GetProperty("lat").GetDouble(), Numbers(got, "lat").Single(), "lat");
    JsonElement search = p.GetProperty("search");
    Equal(search.Str("first_id"), Ids(engine.Search(search.Str("query"), search.U32("limit")))[0], "search");
    JsonElement rev = p.GetProperty("reverse");
    var near = new BlobReader(engine.Reverse(rev.D("lat"), rev.D("lon"), rev.U32("limit")));
    Same(rev.Strings("ids"), near.Strings("id"), "reverse");
    Assert(Math.Abs(Numbers(near, "meters")[0] - rev.D("first_meters")) <= 0.1, "first meters");
    JsonElement admin = p.GetProperty("admin_at");
    Same(admin.Strings("ids"), Ids(engine.AdminAt(admin.D("lat"), admin.D("lon"))), "admin_at");
    Same(p.GetProperty("parent").Strings("ids"), Ids(engine.Parent(p.GetProperty("parent").Str("id"))), "parent");
    Same(p.GetProperty("ancestors").Strings("ids"), Ids(engine.Ancestors(p.GetProperty("ancestors").Str("id"))), "ancestors");
    Same(p.GetProperty("children").Strings("ids"), Ids(engine.Children(p.GetProperty("children").Str("id"))), "children");
    JsonElement nb = p.GetProperty("nearby");
    Same(nb.Strings("ids"), Ids(engine.Nearby(nb.Str("id"), nb.U32("limit"))), "nearby");
    JsonElement w = p.GetProperty("within_radius");
    Same(w.Strings("ids"), Ids(engine.WithinRadius(w.D("lat"), w.D("lon"), w.D("meters"), w.U32("limit"))), "within_radius");
    Code(ErrorCode.InvalidArgument, () => engine.WithinRadius(w.D("lat"), w.D("lon"), -1, 1));
    JsonElement tz = p.GetProperty("tz_at");
    Equal(tz.Str("tz"), new BlobReader(engine.TzAt(tz.D("lat"), tz.D("lon"))).Strings("tz").Single(), "tz_at");
    JsonElement iso = p.GetProperty("by_iso");
    Equal(iso.Str("id"), Ids(engine.ByIso(iso.Str("code"))).Single(), "by_iso");
    Code(ErrorCode.InvalidArgument, () => engine.ByIso(p.Str("bad_iso")));

    JsonElement sw = p.GetProperty("search_with");
    var focus = new SearchOptions { FocusLat = sw.D("focus_lat"), FocusLon = sw.D("focus_lon"), FocusScaleM = sw.D("focus_scale_m") };
    Same(sw.Strings("ids"), Ids(engine.SearchWith(sw.Str("query"), sw.U32("limit"), focus)), "search_with");
    Assert(!sw.Strings("ids").SequenceEqual(Ids(engine.SearchWith(sw.Str("query"), sw.U32("limit")))), "the focus changed nothing");
    Code(ErrorCode.InvalidArgument, () => engine.SearchWith(sw.Str("query"), 1, new SearchOptions { FocusLat = 1.0 }));

    JsonElement pl = p.GetProperty("postal_lookup");
    var hit = new BlobReader(engine.PostalLookup(pl.Str("code"), pl.Str("scheme")));
    Same(pl.Strings("postal"), hit.Strings("postal"), "postal");
    Same(pl.Strings("ids"), hit.Strings("id"), "postal ids");
    JsonElement pp = p.GetProperty("postal_prefix");
    Same(pp.Strings("postal"), new BlobReader(engine.PostalPrefix(pp.Str("input"), pp.Str("scheme"), pp.U32("limit"))).Strings("postal"), "postal_prefix");
    JsonElement pa = p.GetProperty("postal_at");
    var at = new BlobReader(engine.PostalAt(pa.D("lat"), pa.D("lon")));
    Same(pa.Strings("postal"), at.Strings("postal"), "postal_at");
    Same(pa.Strings("code_kind"), at.Strings("code_kind"), "code_kind");
    JsonElement pc = p.GetProperty("postal_codes_of");
    Same(pc.Strings("postal"), new BlobReader(engine.PostalCodesOf(pc.Str("id"))).Strings("postal"), "postal_codes_of");

    Code(ErrorCode.InvalidArgument, () => engine.Get(p.Str("bad_id")));
    Code(ErrorCode.Io, () => engine.OpenFile("/nonexistent.tgd"));
    Code(ErrorCode.Corrupt, () => engine.OpenBytes("not a pack"u8));
    Equal(1u, engine.Layers(), "layers");
    JsonElement h = p.GetProperty("elevation_at");
    Equal(1u, engine.OpenFile(Fixture(h.Str("fixture"))), "dem layer");
    var height = new BlobReader(engine.ElevationAt(h.D("lat"), h.D("lon")));
    Equal(h.Str("source"), height.Strings("source").Single(), "source");
    Equal(h.D("meters"), Numbers(height, "meters").Single(), "meters");
    Assert(engine.Remove(1) && !engine.Remove(1) && engine.Layers() == 1, "remove");
}

void CheckOpenOptions()
{
    JsonElement p = parity.GetProperty("engine");
    JsonElement ow = p.GetProperty("open_with");
    string path = Fixture(p.Str("fixture"));
    byte[] bad = File.ReadAllBytes(path);
    bad[ow.GetProperty("corrupt_at").GetInt32()] ^= 0xff;
    using var scratch = new Engine();
    Code(ErrorCode.Corrupt, () => scratch.OpenBytesWith(bad, new OpenOptions { Verify = Verify.Eager }));
    Equal(0u, scratch.OpenBytesWith(bad), "lazy layer");
    var every = new OpenOptions { Priority = 2, Verify = Verify.Eager, Load = Load.Read, OutlineLevel = 1 };
    Equal(1u, scratch.OpenFileWith(path, every), "layer with every option");
    Code(ErrorCode.InvalidArgument, () => scratch.OpenFileWith(path, new OpenOptions { Verify = (Verify)ow.U32("bad_verify") }));
    Code(ErrorCode.InvalidArgument, () => scratch.OpenFileWith(path, new OpenOptions { OutlineLevel = ow.U32("bad_outline_level") }));

    using var points = new Engine();
    points.OpenFileWith(path, new OpenOptions { Components = ow.Str("components"), Languages = ow.Str("languages") });
    Equal(1, Ids(points.Get(p.GetProperty("get").Str("id"))).Length, "get with point alone");
    Code(ErrorCode.MissingComponent, () => points.Search("Synthland", 1));
    Code(ErrorCode.InvalidArgument, () => points.OpenFileWith(path, new OpenOptions { Components = ow.Str("bad_components") }));
}

void CheckDisposed()
{
    var engine = new Engine();
    engine.Dispose();
    engine.Dispose();
    Throws<ObjectDisposedException>(() => engine.Get(parity.GetProperty("engine").GetProperty("get").Str("id")));
    Throws<ObjectDisposedException>(() => engine.Layers());
}

// One engine from many threads at once, disposed while they run: every call either answers or
// throws ObjectDisposedException; the handle is freed only once no call holds it.
void CheckConcurrent()
{
    JsonElement p = parity.GetProperty("engine");
    JsonElement search = p.GetProperty("search");
    string query = search.Str("query");
    string first = search.Str("first_id");
    uint limit = search.U32("limit");
    var engine = new Engine();
    engine.OpenFile(Fixture(p.Str("fixture")));
    var errors = new System.Collections.Concurrent.ConcurrentBag<Exception>();
    Parallel.For(0, 8, new ParallelOptions { MaxDegreeOfParallelism = 8 }, g =>
    {
        for (int i = 0; i < 300; i++)
        {
            if (g == 0 && i == 200)
            {
                engine.Dispose();
            }
            try
            {
                if (Ids(engine.Search(query, limit))[0] != first)
                {
                    errors.Add(new Exception("wrong first id"));
                    return;
                }
            }
            catch (ObjectDisposedException)
            {
                return;
            }
            catch (Exception e)
            {
                errors.Add(e);
                return;
            }
        }
    });
    engine.Dispose();
    if (!errors.IsEmpty)
    {
        throw new AggregateException(errors);
    }
}

void CheckTz()
{
    JsonElement p = parity.GetProperty("tz");
    using var engine = new Engine();
    engine.OpenFile(Fixture(p.Str("fixture")));
    Equal(p.Str("version"), engine.TzVersion(), "tz version");
    Same(p.Strings("zones"), new BlobReader(engine.TzZones()).Strings("zone"), "zones");
    JsonElement o = p.GetProperty("offset");
    var off = new BlobReader(engine.TzOffsetAt(o.Str("zone"), o.GetProperty("unix").GetInt64()));
    Equal(o.D("seconds"), Numbers(off, "seconds").Single(), "seconds");
    Equal(o.D("dst"), Numbers(off, "dst").Single(), "dst");
    Equal(o.Str("abbr"), off.Strings("abbr").Single(), "abbr");
    foreach (string kind in new[] { "gap", "overlap" })
    {
        JsonElement c = p.GetProperty(kind);
        long[] v = c.GetProperty("civil").EnumerateArray().Select(x => x.GetInt64()).ToArray();
        var l = new BlobReader(engine.TzLocal(c.Str("zone"), (int)v[0], (uint)v[1], (uint)v[2], (uint)v[3], (uint)v[4], (uint)v[5]));
        Equal(kind, l.Strings("kind").Single(), kind);
        Equal($"{c.D("before")},{c.D("after")}", $"{Numbers(l, "before").Single()},{Numbers(l, "after").Single()}", kind + " offsets");
    }
    JsonElement start = p.GetProperty("start");
    Equal(start.D("unix"), Numbers(new BlobReader(engine.TzRulesStart(start.Str("zone"))), "unix").Single(), "start");
    Equal(0, new BlobReader(engine.TzOffsetAt(p.Str("unknown"), 0)).Rows, "unknown zone");
    Code(ErrorCode.InvalidArgument, () => engine.TzLocal(o.Str("zone"), 2024, 13, 1, 0, 0, 0));
}

// The store parity.json describes, or a skip for a library built without one.
Store OpenStore(string mirror)
{
    JsonElement s = parity.GetProperty("store");
    byte[] root = File.ReadAllBytes(Fixture(s.Str("root")));
    string dir = Directory.CreateTempSubdirectory("teigeodata-").FullName;
    try
    {
        Teispace.TeiGeodata.Store.Open(Path.Combine(dir, "missing")).Dispose();
        throw new Exception("a missing store opened");
    }
    catch (TeiGeodataException e) when (e.Code == ErrorCode.MissingComponent)
    {
        throw new SkipException("the library was built without the store");
    }
    catch (TeiGeodataException e) when (e.Code == ErrorCode.Refused)
    {
    }
    Code(ErrorCode.Refused, () => Teispace.TeiGeodata.Store.Create(Path.Combine(dir, "bad"), mirror, mirror, "{}"u8));
    Teispace.TeiGeodata.Store.Create(Path.Combine(dir, "s"), mirror, mirror, root).Dispose();
    return Teispace.TeiGeodata.Store.Open(Path.Combine(dir, "s"));
}

void CheckStore()
{
    JsonElement s = parity.GetProperty("store");
    string pack = s.Str("pack");
    using var store = OpenStore(s.Str("mirror"));
    Equal("", store.Damaged(), "damaged");
    Equal(0, new BlobReader(store.Catalog()).Rows, "catalog");
    Equal(0, new BlobReader(store.Status(false)).Rows, "status");
    using var engine = store.Engine();
    Equal(0u, engine.Layers(), "store engine layers");
    Equal(0, new BlobReader(store.Find(engine, pack, 3)).Rows, "find");
    Equal(0, new BlobReader(store.Locate(engine, 27.7, 85.3, 1000)).Rows, "locate");
    Code(ErrorCode.InvalidArgument, () => store.Find(engine, pack, 10001));
    Code(ErrorCode.InvalidArgument, () => store.Locate(engine, 91, 0, 1000));
    Code(ErrorCode.InvalidArgument, () => store.Install(pack, new InstallOptions { Components = s.Str("bad_components") }));
    Code(ErrorCode.Refused, () => store.Remove(pack));
    Code(ErrorCode.InvalidArgument, () => store.InstallPass(pack, ""));
    using (Job pass = store.InstallPass(pack, Directory.CreateTempSubdirectory("teigeodata-").FullName))
    {
        Code(ErrorCode.Network, () => pass.Result());
    }
    // The awaitable forms run on threads of their own and carry failures as exceptions.
    Equal(0, new BlobReader(store.CatalogAsync().GetAwaiter().GetResult()).Rows, "async catalog");
    string missing = Path.Combine(Path.GetTempPath(), "teigeodata-missing-" + Guid.NewGuid().ToString("N"));
    Code(ErrorCode.Refused, () => Teispace.TeiGeodata.Store.OpenAsync(missing).GetAwaiter().GetResult().Dispose());
    using Job job = store.Refresh(pack);
    Code(ErrorCode.Network, () => job.ResultAsync().GetAwaiter().GetResult());
    Assert(job.IsEnded() && job.WaitProgress(job.Seq(), 60000) == job.Seq() && job.DoneBytes() == job.TotalBytes(), "job state");
    job.Cancel();
}

// A job's progress moves while a mirror trickles bytes, and a token cancelled during
// ResultAsync ends the job at once (D-283).
void CheckJobCancel()
{
    using var listener = new TcpListener(IPAddress.Loopback, 0);
    listener.Start();
    using var serving = new CancellationTokenSource();
    Task server = Trickle(listener, serving.Token);
    try
    {
        string mirror = $"http://127.0.0.1:{((IPEndPoint)listener.LocalEndpoint).Port}/";
        using Store store = OpenStore(mirror);

        // Progress ends with the job, at its total.
        using (Store quickStore = OpenStore(parity.GetProperty("store").Str("mirror")))
        using (Job quick = quickStore.Refresh(parity.GetProperty("store").Str("pack")))
        {
            JobProgress last = default;
            foreach (JobProgress p in Sync(quick.ProgressAsync()))
            {
                last = p;
            }
            Assert(last.Done == last.Total && quick.IsEnded(), $"progress ended at {last}");
        }

        using Job job = store.Refresh(parity.GetProperty("store").Str("pack"));
        using var cancel = new CancellationTokenSource();
        Task<byte[]> result = job.ResultAsync(cancel.Token);
        int moves = 0;
        foreach (JobProgress _ in Sync(job.ProgressAsync()))
        {
            if (++moves == 3)
            {
                break;
            }
        }
        Equal(3, moves, "progress moves");
        var clock = Stopwatch.StartNew();
        cancel.Cancel();
        Assert(Task.WhenAny(result, Task.Delay(TimeSpan.FromSeconds(5))).GetAwaiter().GetResult() == result, "the cancel did not end the job");
        var e = Throws<TeiGeodataException>(() => result.GetAwaiter().GetResult());
        Equal(ErrorCode.Cancelled, e.Code, "cancelled code");
        Assert(clock.Elapsed < TimeSpan.FromSeconds(2) && job.IsEnded(), $"cancel took {clock.Elapsed}");
    }
    finally
    {
        serving.Cancel();
        listener.Stop();
        try
        {
            server.Wait(TimeSpan.FromSeconds(5));
        }
        catch (AggregateException)
        {
        }
    }
}

// A malformed blob is an InvalidDataException (or a missing column, a wrong type), never a
// crash or a read outside it: mutations of a real blob, from a fixed seed.
void CheckReaderFuzz()
{
    byte[] blob = Geodata.SampleRows(5);
    var random = new Random(20261114);
    for (int round = 0; round < 50_000; round++)
    {
        byte[] data = round % 7 == 0 ? blob[..random.Next(blob.Length)] : (byte[])blob.Clone();
        for (int flips = random.Next(1, 4); flips > 0 && data.Length > 0; flips--)
        {
            data[random.Next(data.Length)] = (byte)random.Next(256);
        }
        BlobReader r;
        try
        {
            r = new BlobReader(data);
        }
        catch (InvalidDataException)
        {
            continue;
        }
        foreach (string name in r.Columns)
        {
            Tolerate(() => r.Strings(name));
            Tolerate(() => r.Nulls(name));
            Tolerate(() => r.Values<double>(name).ToArray());
            Tolerate(() => r.Values<byte>(name).ToArray());
        }
    }

    static void Tolerate(Action read)
    {
        try
        {
            read();
        }
        catch (Exception e) when (e is InvalidDataException or InvalidCastException)
        {
        }
    }
}

// ---- helpers ----

static string FindBindings()
{
    foreach (string start in new[] { AppContext.BaseDirectory, Directory.GetCurrentDirectory() })
    {
        for (string? dir = start; dir is not null; dir = Path.GetDirectoryName(dir))
        {
            foreach (string candidate in new[] { dir, Path.Combine(dir, "bindings") })
            {
                if (File.Exists(Path.Combine(candidate, "parity.json")) && Directory.Exists(Path.Combine(candidate, "fixtures")))
                {
                    return candidate;
                }
            }
        }
    }
    throw new FileNotFoundException("bindings/parity.json not found above this program or the current directory");
}

static string[] Ids(byte[] blob) => new BlobReader(blob).Strings("id");

static double[] Numbers(BlobReader r, string name)
{
    if (!r.TryGetType(name, out ColumnType type))
    {
        throw new Exception($"no column {name}");
    }
    return type switch
    {
        ColumnType.F64 => r.Values<double>(name).ToArray(),
        ColumnType.F32 => Widen(r.Values<float>(name)),
        ColumnType.I32 => Widen(r.Values<int>(name)),
        ColumnType.I64 => Widen(r.Values<long>(name)),
        ColumnType.I16 => Widen(r.Values<short>(name)),
        ColumnType.U8 => Widen(r.Values<byte>(name)),
        ColumnType.U16 => Widen(r.Values<ushort>(name)),
        ColumnType.U32 => Widen(r.Values<uint>(name)),
        ColumnType.U64 => Widen(r.Values<ulong>(name)),
        _ => throw new Exception($"{name} is not a number column"),
    };
}

static double[] Widen<T>(ReadOnlySpan<T> values)
    where T : unmanaged, System.Numerics.INumberBase<T>
{
    var output = new double[values.Length];
    for (int i = 0; i < values.Length; i++)
    {
        output[i] = double.CreateChecked(values[i]);
    }
    return output;
}

static void Assert(bool condition, string what)
{
    if (!condition)
    {
        throw new Exception(what);
    }
}

static void Equal<T>(T want, T got, string what)
{
    if (!EqualityComparer<T>.Default.Equals(want, got))
    {
        throw new Exception($"{what}: got {got}, want {want}");
    }
}

static void Same(string[] want, string[] got, string what)
{
    if (!want.SequenceEqual(got))
    {
        throw new Exception($"{what}: got [{string.Join(", ", got)}], want [{string.Join(", ", want)}]");
    }
}

static TException Throws<TException>(Action call)
    where TException : Exception
{
    try
    {
        call();
    }
    catch (TException e)
    {
        return e;
    }
    throw new Exception($"no {typeof(TException).Name} was thrown");
}

static TeiGeodataException Code(ErrorCode code, Action call)
{
    var e = Throws<TeiGeodataException>(call);
    if (e.Code != code)
    {
        throw new Exception($"want {code}, got {e.Code}: {e.Message}");
    }
    return e;
}

static IEnumerable<T> Sync<T>(IAsyncEnumerable<T> items)
{
    IAsyncEnumerator<T> e = items.GetAsyncEnumerator();
    try
    {
        while (e.MoveNextAsync().AsTask().GetAwaiter().GetResult())
        {
            yield return e.Current;
        }
    }
    finally
    {
        e.DisposeAsync().AsTask().GetAwaiter().GetResult();
    }
}

// A mirror that answers every request with 1,000 bytes, one each 20 ms.
static async Task Trickle(TcpListener listener, CancellationToken stop)
{
    var clients = new List<Task>();
    try
    {
        while (!stop.IsCancellationRequested)
        {
            TcpClient client = await listener.AcceptTcpClientAsync(stop).ConfigureAwait(false);
            clients.Add(Serve(client, stop));
        }
    }
    catch (Exception e) when (e is OperationCanceledException or SocketException or ObjectDisposedException)
    {
    }
    await Task.WhenAll(clients).ConfigureAwait(false);

    static async Task Serve(TcpClient client, CancellationToken stop)
    {
        using (client)
        {
            try
            {
                NetworkStream stream = client.GetStream();
                var request = new byte[8192];
                int read = 0;
                while (read < request.Length && !Encoding.ASCII.GetString(request, 0, read).Contains("\r\n\r\n"))
                {
                    int n = await stream.ReadAsync(request.AsMemory(read), stop).ConfigureAwait(false);
                    if (n == 0)
                    {
                        return;
                    }
                    read += n;
                }
                await stream.WriteAsync("HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n"u8.ToArray(), stop).ConfigureAwait(false);
                for (int i = 0; i < 1000; i++)
                {
                    await stream.WriteAsync(" "u8.ToArray(), stop).ConfigureAwait(false);
                    await stream.FlushAsync(stop).ConfigureAwait(false);
                    await Task.Delay(20, stop).ConfigureAwait(false);
                }
            }
            catch (Exception e) when (e is OperationCanceledException or IOException or SocketException)
            {
            }
        }
    }
}

sealed class SkipException(string message) : Exception(message);

static class Json
{
    public static string Str(this JsonElement e, string name) => e.GetProperty(name).GetString()!;

    public static uint U32(this JsonElement e, string name) => e.GetProperty(name).GetUInt32();

    public static double D(this JsonElement e, string name) => e.GetProperty(name).GetDouble();

    public static string[] Strings(this JsonElement e, string name) =>
        e.GetProperty(name).EnumerateArray().Select(x => x.GetString()!).ToArray();

    public static double[] Doubles(this JsonElement e, string name) =>
        e.GetProperty(name).EnumerateArray().Select(x => x.GetDouble()).ToArray();
}
