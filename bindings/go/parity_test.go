package teigeodata_test

// Parity: the Go binding gives the results bindings/parity.json expects (checked there against
// Rust). Run with the library from the C package or a build of teigeodata-c:
//
//	TEIGEODATA_LIB=../../target/dist/libteigeodata.dylib go test ./...
//
// and the same with CGO_ENABLED=0 for the purego layer.

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	tg "github.com/teispace/teigeodata/bindings/go"
)

var parity struct {
	Version    string
	Components string
	Fold       [][2]string
	SampleRows struct {
		Rows         uint32
		Bytes        int
		Columns      []string
		LatSum       float64 `json:"lat_sum"`
		ElevationSum float64 `json:"elevation_sum"`
		LastName     string  `json:"last_name"`
	} `json:"sample_rows"`
	SampleRowsMax uint32 `json:"sample_rows_max"`
	Progress      struct {
		Steps            uint32
		CancelAfter      uint32 `json:"cancel_after"`
		CancelledMessage string `json:"cancelled_message"`
	}
	DistanceM struct {
		Args       [4]float64
		Metres     float64
		OutOfRange [4]float64 `json:"out_of_range"`
	} `json:"distance_m"`
	BearingDeg struct {
		Args       [4]float64
		Degrees    float64
		OutOfRange [4]float64 `json:"out_of_range"`
	} `json:"bearing_deg"`
	ErrorCodes map[string]int32 `json:"error_codes"`
	Store      struct {
		Root, Mirror, Pack string
		BadComponents      string `json:"bad_components"`
	}
	Engine struct {
		Fixture string
		Columns []string
		Get     struct {
			ID, Name, Kind string
			Lat, Lon       float64
		}
		Search struct {
			Query   string
			Limit   uint32
			FirstID string `json:"first_id"`
		}
		SearchWith struct {
			Query       string
			Limit       uint32
			FocusLat    float64 `json:"focus_lat"`
			FocusLon    float64 `json:"focus_lon"`
			FocusScaleM float64 `json:"focus_scale_m"`
			IDs         []string
		} `json:"search_with"`
		OpenWith struct {
			CorruptAt       int    `json:"corrupt_at"`
			BadVerify       uint32 `json:"bad_verify"`
			BadOutlineLevel uint32 `json:"bad_outline_level"`
			Components      string
			Languages       string
			BadComponents   string `json:"bad_components"`
		} `json:"open_with"`
		Reverse struct {
			Lat, Lon    float64
			Limit       uint32
			IDs         []string
			FirstMeters float64 `json:"first_meters"`
		}
		AdminAt struct {
			Lat, Lon float64
			IDs      []string
		} `json:"admin_at"`
		Parent, Ancestors, Children struct {
			ID  string
			IDs []string
		}
		Nearby struct {
			ID    string
			Limit uint32
			IDs   []string
		}
		WithinRadius struct {
			Lat, Lon, Meters float64
			Limit            uint32
			IDs              []string
		} `json:"within_radius"`
		TzAt struct {
			Lat, Lon float64
			Tz       string
		} `json:"tz_at"`
		ByIso struct {
			Code, ID string
		} `json:"by_iso"`
		PostalLookup struct {
			Code, Scheme string
			Postal, IDs  []string
		} `json:"postal_lookup"`
		PostalPrefix struct {
			Input, Scheme string
			Limit         uint32
			Postal        []string
		} `json:"postal_prefix"`
		PostalAt struct {
			Lat, Lon float64
			Postal   []string
			CodeKind []string `json:"code_kind"`
		} `json:"postal_at"`
		PostalCodesOf struct {
			ID     string
			Postal []string
		} `json:"postal_codes_of"`
		ElevationAt struct {
			Fixture  string
			Lat, Lon float64
			Meters   float64
			Source   string
		} `json:"elevation_at"`
		Boundary struct {
			Fixture, ID string
			Lod         uint32
			Columns     []string
			ToleranceM  float64 `json:"tolerance_m"`
			Vertices    float64
			GeoJSON     string `json:"geojson"`
			UnknownID   string `json:"unknown_id"`
			BadLod      uint32 `json:"bad_lod"`
		}
		Distance struct {
			From, To        string
			Meters, Bearing float64
		}
		BadID  string `json:"bad_id"`
		BadIso string `json:"bad_iso"`
	}
	Tz struct {
		Fixture  string
		Version  string
		Embedded struct {
			Fixture        string
			Offset, Length uint64
			PastEnd        uint64 `json:"past_end"`
		}
		Zones  []string
		Offset struct {
			Zone    string
			Unix    int64
			Seconds float64
			Dst     float64
			Abbr    string
		}
		Gap, Overlap struct {
			Zone          string
			Civil         [6]int64
			Before, After float64
		}
		Start struct {
			Zone string
			Unix float64
		}
		Unknown string
	}
}

func TestMain(m *testing.M) {
	// TEIGEODATA_PARITY names another copy (cargo xtask parity-mutants plants a difference in one).
	path := os.Getenv("TEIGEODATA_PARITY")
	if path == "" {
		path = "../parity.json"
	}
	data, err := os.ReadFile(path)
	if err == nil {
		err = json.Unmarshal(data, &parity)
	}
	if err != nil {
		panic(err)
	}
	os.Exit(m.Run())
}

func fixture(rel string) string { return filepath.Join("..", rel) }

// must returns the value, or panics (failing the test) on an error.
func must[T any](v T, err error) T {
	if err != nil {
		panic(err)
	}
	return v
}

// wantCode fails unless err carries code.
func wantCode(t *testing.T, err error, code tg.ErrorCode) {
	t.Helper()
	var e *tg.Error
	if !errors.As(err, &e) || e.Code != code || !errors.Is(err, code) {
		t.Fatalf("want %v, got %v", code, err)
	}
}

func reader(blob []byte, err error) *tg.Reader { return must(tg.NewReader(must(blob, err))) }

// column reads the string column called name from a call's blob: column("id")(engine.Get(id)).
func column(name string) func([]byte, error) []string {
	return func(blob []byte, err error) []string { return must(reader(blob, err).Strings(name)) }
}

// numbers is a number column of any type, as float64.
func numbers(t *testing.T, r *tg.Reader, name string) []float64 {
	t.Helper()
	ty, ok := r.Type(name)
	if !ok {
		t.Fatalf("no column %s", name)
	}
	var out []float64
	switch ty {
	case tg.TypeF64:
		out = append(out, must(tg.Values[float64](r, name))...)
	case tg.TypeF32:
		for _, v := range must(tg.Values[float32](r, name)) {
			out = append(out, float64(v))
		}
	case tg.TypeI32:
		for _, v := range must(tg.Values[int32](r, name)) {
			out = append(out, float64(v))
		}
	case tg.TypeI64:
		for _, v := range must(tg.Values[int64](r, name)) {
			out = append(out, float64(v))
		}
	case tg.TypeI16:
		for _, v := range must(tg.Values[int16](r, name)) {
			out = append(out, float64(v))
		}
	case tg.TypeU8:
		for _, v := range must(tg.Values[uint8](r, name)) {
			out = append(out, float64(v))
		}
	case tg.TypeU16:
		for _, v := range must(tg.Values[uint16](r, name)) {
			out = append(out, float64(v))
		}
	case tg.TypeU32:
		for _, v := range must(tg.Values[uint32](r, name)) {
			out = append(out, float64(v))
		}
	case tg.TypeU64:
		for _, v := range must(tg.Values[uint64](r, name)) {
			out = append(out, float64(v))
		}
	default:
		t.Fatalf("%s is not a number column", name)
	}
	return out
}

func equal[T comparable](t *testing.T, what string, got, want []T) {
	t.Helper()
	if !slices.Equal(got, want) {
		t.Fatalf("%s: got %v, want %v", what, got, want)
	}
}

func TestVersion(t *testing.T) {
	if got := must(tg.Version()); got != parity.Version {
		t.Fatalf("version %q, want %q", got, parity.Version)
	}
	if got := must(tg.Components()); got != parity.Components {
		t.Fatalf("components %q", got)
	}
	if err := tg.Noop(); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(tg.LibraryPath(), "teigeodata") {
		t.Fatalf("library path %q", tg.LibraryPath())
	}
	if err := tg.LoadLibrary(tg.LibraryPath()); err != nil {
		t.Fatal("loading the same library again:", err)
	}
	if err := tg.LoadLibrary("/elsewhere/libteigeodata.so"); err == nil {
		t.Fatal("a second library was accepted")
	}
}

func TestErrorCodes(t *testing.T) {
	for name, value := range parity.ErrorCodes {
		code := tg.ErrorCode(value)
		if code.String() != name {
			t.Errorf("code %d is %q, want %q", value, code, name)
		}
	}
	if tg.ErrorCode(99).String() != "ErrorCode(99)" {
		t.Error(tg.ErrorCode(99).String())
	}
}

func TestFold(t *testing.T) {
	for _, c := range parity.Fold {
		if got := must(tg.Fold(c[0])); got != c[1] {
			t.Errorf("fold(%q) = %q, want %q", c[0], got, c[1])
		}
	}
	_, err := tg.Fold("\xff")
	wantCode(t, err, tg.CodeInvalidArgument)
}

func TestSampleRows(t *testing.T) {
	s := parity.SampleRows
	blob := must(tg.SampleRows(s.Rows))
	if len(blob) != s.Bytes {
		t.Fatalf("%d bytes, want %d", len(blob), s.Bytes)
	}
	r := must(tg.NewReader(blob))
	equal(t, "columns", r.Columns(), s.Columns)
	lat := 0.0
	for _, v := range must(tg.Values[float64](r, "lat")) {
		lat += v
	}
	elevation := 0.0
	for _, v := range numbers(t, r, "elevation") {
		elevation += v
	}
	if lat != s.LatSum || elevation != s.ElevationSum {
		t.Fatalf("sums %v %v", lat, elevation)
	}
	if names := must(r.Strings("name")); names[len(names)-1] != s.LastName {
		t.Fatal(names[len(names)-1])
	}
	if _, err := tg.Values[float32](r, "lat"); err == nil {
		t.Fatal("a float64 column read as float32")
	}
	if tg.SampleRowsMax != parity.SampleRowsMax {
		t.Fatal(tg.SampleRowsMax)
	}
	_, err := tg.SampleRows(tg.SampleRowsMax + 1)
	wantCode(t, err, tg.CodeInvalidArgument)
	if !strings.Contains(err.Error(), "at most") {
		t.Fatal(err)
	}
}

func TestDistance(t *testing.T) {
	d := parity.DistanceM
	got := must(tg.DistanceM(d.Args[0], d.Args[1], d.Args[2], d.Args[3]))
	if math.Abs(got-d.Metres) > 1e-6 {
		t.Fatal(got)
	}
	o := d.OutOfRange
	_, err := tg.DistanceM(o[0], o[1], o[2], o[3])
	wantCode(t, err, tg.CodeInvalidArgument)
}

func TestBearing(t *testing.T) {
	d := parity.BearingDeg
	got := must(tg.BearingDeg(d.Args[0], d.Args[1], d.Args[2], d.Args[3]))
	if math.Abs(got-d.Degrees) > 1e-9 {
		t.Fatal(got)
	}
	o := d.OutOfRange
	_, err := tg.BearingDeg(o[0], o[1], o[2], o[3])
	wantCode(t, err, tg.CodeInvalidArgument)
}

func TestBoundaryAndDistance(t *testing.T) {
	p := parity.Engine
	c := p.Boundary
	outlined := must(tg.NewEngine())
	defer outlined.Close()
	must(outlined.OpenFile(fixture(c.Fixture)))
	got := reader(outlined.Boundary(c.ID, c.Lod))
	equal(t, "boundary columns", got.Columns(), c.Columns)
	equal(t, "boundary id", must(got.Strings("id")), []string{c.ID})
	equal(t, "boundary geojson", must(got.Strings("geojson")), []string{c.GeoJSON})
	equal(t, "boundary lod", numbers(t, got, "lod"), []float64{float64(c.Lod)})
	equal(t, "boundary tolerance", numbers(t, got, "tolerance_m"), []float64{c.ToleranceM})
	equal(t, "boundary vertices", numbers(t, got, "vertices"), []float64{c.Vertices})
	if n := reader(outlined.Boundary(c.UnknownID, 0)).Rows(); n != 0 {
		t.Fatal("boundary of an unknown ID", n)
	}
	_, err := outlined.Boundary(c.ID, c.BadLod)
	wantCode(t, err, tg.CodeInvalidArgument)

	d := p.Distance
	engine := must(tg.NewEngine())
	defer engine.Close()
	must(engine.OpenFile(fixture(p.Fixture)))
	m := reader(engine.Distance(d.From, d.To))
	if math.Abs(numbers(t, m, "meters")[0]-d.Meters) > 1e-6 || math.Abs(numbers(t, m, "bearing")[0]-d.Bearing) > 1e-6 {
		t.Fatal("distance", numbers(t, m, "meters"), numbers(t, m, "bearing"))
	}
	if n := reader(engine.Distance(d.From, c.UnknownID)).Rows(); n != 0 {
		t.Fatal("distance to an unknown ID", n)
	}
}

func TestProgress(t *testing.T) {
	var seen []uint32
	steps := must(tg.Progress(3, func(s uint32) bool { seen = append(seen, s); return true }))
	if steps != 3 {
		t.Fatal(steps)
	}
	equal(t, "steps", seen, []uint32{1, 2, 3})
	g := parity.Progress
	_, err := tg.Progress(g.Steps, func(s uint32) bool { return s < g.CancelAfter })
	wantCode(t, err, tg.CodeCancelled)
	if !strings.Contains(err.Error(), g.CancelledMessage) {
		t.Fatal(err)
	}
	_, err = tg.Progress(1, nil)
	wantCode(t, err, tg.CodeInvalidArgument)

	boom := errors.New("boom")
	func() {
		defer func() {
			if got := recover(); got != boom {
				t.Fatalf("recovered %v, want the callback's panic", got)
			}
		}()
		_, _ = tg.Progress(5, func(uint32) bool { panic(boom) })
		t.Fatal("the panic was lost")
	}()
	// The library is still sound after a panic in a callback.
	if steps := must(tg.Progress(2, func(uint32) bool { return true })); steps != 2 {
		t.Fatal(steps)
	}
}

func TestEngine(t *testing.T) {
	p := parity.Engine
	engine := must(tg.NewEngine())
	defer engine.Close()
	if layer := must(engine.OpenFile(fixture(p.Fixture))); layer != 0 {
		t.Fatal(layer)
	}
	got := reader(engine.Get(p.Get.ID))
	equal(t, "name", must(got.Strings("name")), []string{p.Get.Name})
	equal(t, "kind", must(got.Strings("kind")), []string{p.Get.Kind})
	equal(t, "columns", got.Columns(), p.Columns)
	equal(t, "lat", numbers(t, got, "lat"), []float64{p.Get.Lat})
	equal(t, "lon", numbers(t, got, "lon"), []float64{p.Get.Lon})
	if first := column("id")(engine.Search(p.Search.Query, p.Search.Limit))[0]; first != p.Search.FirstID {
		t.Fatal(first)
	}
	near := reader(engine.Reverse(p.Reverse.Lat, p.Reverse.Lon, p.Reverse.Limit))
	equal(t, "reverse", must(near.Strings("id")), p.Reverse.IDs)
	if m := numbers(t, near, "meters")[0]; math.Abs(m-p.Reverse.FirstMeters) > 0.1 {
		t.Fatal(m)
	}
	equal(t, "admin_at", column("id")(engine.AdminAt(p.AdminAt.Lat, p.AdminAt.Lon)), p.AdminAt.IDs)
	equal(t, "parent", column("id")(engine.Parent(p.Parent.ID)), p.Parent.IDs)
	equal(t, "ancestors", column("id")(engine.Ancestors(p.Ancestors.ID)), p.Ancestors.IDs)
	equal(t, "children", column("id")(engine.Children(p.Children.ID)), p.Children.IDs)
	equal(t, "nearby", column("id")(engine.Nearby(p.Nearby.ID, p.Nearby.Limit)), p.Nearby.IDs)
	w := p.WithinRadius
	equal(t, "within_radius", column("id")(engine.WithinRadius(w.Lat, w.Lon, w.Meters, w.Limit)), w.IDs)
	_, err := engine.WithinRadius(w.Lat, w.Lon, -1, 1)
	wantCode(t, err, tg.CodeInvalidArgument)
	equal(t, "tz_at", column("tz")(engine.TzAt(p.TzAt.Lat, p.TzAt.Lon)), []string{p.TzAt.Tz})
	equal(t, "by_iso", column("id")(engine.ByIso(p.ByIso.Code)), []string{p.ByIso.ID})
	_, err = engine.ByIso(p.BadIso)
	wantCode(t, err, tg.CodeInvalidArgument)

	sw := p.SearchWith
	focus := &tg.SearchOptions{FocusLat: &sw.FocusLat, FocusLon: &sw.FocusLon, FocusScaleM: &sw.FocusScaleM}
	equal(t, "search_with", column("id")(engine.SearchWith(sw.Query, sw.Limit, focus)), sw.IDs)
	if slices.Equal(column("id")(engine.SearchWith(sw.Query, sw.Limit, nil)), sw.IDs) {
		t.Fatal("the focus changed nothing")
	}
	_, err = engine.SearchWith(sw.Query, 1, &tg.SearchOptions{FocusLat: new(1.0)})
	wantCode(t, err, tg.CodeInvalidArgument)

	pl := p.PostalLookup
	hit := reader(engine.PostalLookup(pl.Code, pl.Scheme))
	equal(t, "postal", must(hit.Strings("postal")), pl.Postal)
	equal(t, "postal ids", must(hit.Strings("id")), pl.IDs)
	pp := p.PostalPrefix
	equal(t, "postal_prefix", column("postal")(engine.PostalPrefix(pp.Input, pp.Scheme, pp.Limit)), pp.Postal)
	pa := reader(engine.PostalAt(p.PostalAt.Lat, p.PostalAt.Lon))
	equal(t, "postal_at", must(pa.Strings("postal")), p.PostalAt.Postal)
	equal(t, "code_kind", must(pa.Strings("code_kind")), p.PostalAt.CodeKind)
	equal(t, "postal_codes_of", column("postal")(engine.PostalCodesOf(p.PostalCodesOf.ID)), p.PostalCodesOf.Postal)

	_, err = engine.Get(p.BadID)
	wantCode(t, err, tg.CodeInvalidArgument)
	_, err = engine.OpenFile("/nonexistent.tgd")
	wantCode(t, err, tg.CodeIo)
	_, err = engine.OpenBytes([]byte("not a pack"))
	wantCode(t, err, tg.CodeCorrupt)
	if engine.Layers() != 1 {
		t.Fatal(engine.Layers())
	}
	h := p.ElevationAt
	if layer := must(engine.OpenFile(fixture(h.Fixture))); layer != 1 {
		t.Fatal(layer)
	}
	height := reader(engine.ElevationAt(h.Lat, h.Lon))
	equal(t, "source", must(height.Strings("source")), []string{h.Source})
	equal(t, "meters", numbers(t, height, "meters"), []float64{h.Meters})
	if !engine.Remove(1) || engine.Remove(1) || engine.Layers() != 1 {
		t.Fatal("remove")
	}
}

func TestOpenOptions(t *testing.T) {
	p := parity.Engine
	ow := p.OpenWith
	path := fixture(p.Fixture)
	bad := must(os.ReadFile(path))
	bad[ow.CorruptAt] ^= 0xff
	scratch := must(tg.NewEngine())
	defer scratch.Close()
	_, err := scratch.OpenBytesWith(bad, &tg.OpenOptions{Verify: tg.VerifyEager})
	wantCode(t, err, tg.CodeCorrupt)
	if layer := must(scratch.OpenBytesWith(bad, nil)); layer != 0 {
		t.Fatal(layer)
	}
	every := &tg.OpenOptions{Priority: 2, Verify: tg.VerifyEager, Load: tg.LoadRead, OutlineLevel: new(uint32(1))}
	if layer := must(scratch.OpenFileWith(path, every)); layer != 1 {
		t.Fatal(layer)
	}
	_, err = scratch.OpenFileWith(path, &tg.OpenOptions{Verify: tg.Verify(ow.BadVerify)})
	wantCode(t, err, tg.CodeInvalidArgument)
	_, err = scratch.OpenFileWith(path, &tg.OpenOptions{OutlineLevel: &ow.BadOutlineLevel})
	wantCode(t, err, tg.CodeInvalidArgument)

	points := must(tg.NewEngine())
	defer points.Close()
	// Text options built at run time: pinned for the call (cgo checks this).
	languages := strings.Clone(ow.Languages)
	must(points.OpenFileWith(path, &tg.OpenOptions{Components: ow.Components, Languages: languages}))
	if ids := column("id")(points.Get(p.Get.ID)); len(ids) != 1 {
		t.Fatal(ids)
	}
	_, err = points.Search("Synthland", 1)
	wantCode(t, err, tg.CodeMissingComponent)
	_, err = points.OpenFileWith(path, &tg.OpenOptions{Components: ow.BadComponents})
	wantCode(t, err, tg.CodeInvalidArgument)
}

func TestClosed(t *testing.T) {
	engine := must(tg.NewEngine())
	engine.Close()
	engine.Close()
	_, err := engine.Get(parity.Engine.Get.ID)
	wantCode(t, err, tg.CodeInvalidArgument)
	defer func() {
		if recover() == nil {
			t.Fatal("Layers on a closed engine did not panic")
		}
	}()
	engine.Layers()
}

// One engine from many goroutines at once, closed while they run: every call either answers or
// fails with CodeInvalidArgument once the engine is closed (run with -race).
func TestConcurrent(t *testing.T) {
	p := parity.Engine
	engine := must(tg.NewEngine())
	must(engine.OpenFile(fixture(p.Fixture)))
	var wg sync.WaitGroup
	var closed sync.Once
	for g := range 8 {
		wg.Go(func() {
			for i := range 300 {
				if g == 0 && i == 200 {
					closed.Do(engine.Close)
				}
				blob, err := engine.Search(p.Search.Query, p.Search.Limit)
				if err != nil {
					if !errors.Is(err, tg.CodeInvalidArgument) {
						t.Error(err)
					}
					return
				}
				if ids := column("id")(blob, nil); ids[0] != p.Search.FirstID {
					t.Error(ids)
					return
				}
			}
		})
	}
	wg.Wait()
	closed.Do(engine.Close)
}

func TestTz(t *testing.T) {
	p := parity.Tz
	engine := must(tg.NewEngine())
	defer engine.Close()
	must(engine.OpenFile(fixture(p.Fixture)))
	if v := engine.TzVersion(); v != p.Version {
		t.Fatal(v)
	}
	// The same pack inside another file, opened from its range (an APK asset).
	e := p.Embedded
	embedded := must(tg.NewEngine())
	defer embedded.Close()
	must(embedded.OpenFileRange(fixture(e.Fixture), e.Offset, e.Length))
	if v := embedded.TzVersion(); v != p.Version {
		t.Fatal("embedded", v)
	}
	_, err := embedded.OpenFileRange(fixture(e.Fixture), e.Offset, e.PastEnd)
	wantCode(t, err, tg.CodeIo)
	_, err = embedded.OpenFileRange(fixture(e.Fixture), e.Offset-1, e.Length)
	wantCode(t, err, tg.CodeCorrupt)
	equal(t, "zones", column("zone")(engine.TzZones()), p.Zones)
	o := p.Offset
	off := reader(engine.TzOffsetAt(o.Zone, o.Unix))
	equal(t, "seconds", numbers(t, off, "seconds"), []float64{o.Seconds})
	equal(t, "dst", numbers(t, off, "dst"), []float64{o.Dst})
	equal(t, "abbr", must(off.Strings("abbr")), []string{o.Abbr})
	for kind, c := range map[string]struct {
		Zone          string
		Civil         [6]int64
		Before, After float64
	}{"gap": p.Gap, "overlap": p.Overlap} {
		v := c.Civil
		l := reader(engine.TzLocal(c.Zone, int32(v[0]), uint32(v[1]), uint32(v[2]), uint32(v[3]), uint32(v[4]), uint32(v[5])))
		equal(t, kind, must(l.Strings("kind")), []string{kind})
		equal(t, kind+" offsets", append(numbers(t, l, "before"), numbers(t, l, "after")...), []float64{c.Before, c.After})
	}
	equal(t, "start", numbers(t, reader(engine.TzRulesStart(p.Start.Zone)), "unix"), []float64{p.Start.Unix})
	if n := reader(engine.TzOffsetAt(p.Unknown, 0)).Rows(); n != 0 {
		t.Fatal(n)
	}
	_, err = engine.TzLocal(o.Zone, 2024, 13, 1, 0, 0, 0)
	wantCode(t, err, tg.CodeInvalidArgument)
}

// openStore is the store parity.json describes, or a skip for a library built without one.
func openStore(t *testing.T, mirror string) *tg.Store {
	t.Helper()
	s := parity.Store
	root := must(os.ReadFile(fixture(s.Root)))
	dir := t.TempDir()
	_, err := tg.OpenStore(filepath.Join(dir, "missing"))
	if errors.Is(err, tg.CodeMissingComponent) {
		t.Skip("the library was built without the store")
	}
	wantCode(t, err, tg.CodeRefused)
	_, err = tg.CreateStore(filepath.Join(dir, "bad"), mirror, mirror, []byte("{}"))
	wantCode(t, err, tg.CodeRefused)
	must(tg.CreateStore(filepath.Join(dir, "s"), mirror, mirror, root)).Close()
	return must(tg.OpenStore(filepath.Join(dir, "s")))
}

func TestStore(t *testing.T) {
	s := parity.Store
	store := openStore(t, s.Mirror)
	defer store.Close()
	if d := store.Damaged(); d != "" {
		t.Fatal(d)
	}
	if n := reader(store.Catalog()).Rows(); n != 0 {
		t.Fatal(n)
	}
	if n := reader(store.Status(false)).Rows(); n != 0 {
		t.Fatal(n)
	}
	engine := must(store.Engine())
	defer engine.Close()
	if engine.Layers() != 0 {
		t.Fatal(engine.Layers())
	}
	if n := reader(store.Find(engine, s.Pack, 3)).Rows(); n != 0 {
		t.Fatal(n)
	}
	if n := reader(store.Locate(engine, 27.7, 85.3, 1000)).Rows(); n != 0 {
		t.Fatal(n)
	}
	_, err := store.Find(engine, s.Pack, 10001)
	wantCode(t, err, tg.CodeInvalidArgument)
	_, err = store.Locate(engine, 91, 0, 1000)
	wantCode(t, err, tg.CodeInvalidArgument)
	_, err = store.Install(s.Pack, &tg.InstallOptions{Components: s.BadComponents})
	wantCode(t, err, tg.CodeInvalidArgument)
	_, err = store.Remove(s.Pack)
	wantCode(t, err, tg.CodeRefused)
	_, err = store.InstallPass(s.Pack, "", nil)
	wantCode(t, err, tg.CodeInvalidArgument)
	pass := must(store.InstallPass(s.Pack, t.TempDir(), nil))
	_, err = pass.Result()
	wantCode(t, err, tg.CodeNetwork)
	pass.Close()
	job := store.Refresh(s.Pack)
	defer job.Close()
	_, err = job.Result()
	wantCode(t, err, tg.CodeNetwork)
	if !job.IsEnded() || job.WaitProgress(job.Seq(), 60000) != job.Seq() || job.DoneBytes() != job.TotalBytes() {
		t.Fatal("job state")
	}
	job.Cancel()
}

// A job's progress moves while a mirror trickles bytes, and a context cancelled during Wait
// ends the job at once (D-283).
func TestJobCancel(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Length", "1000")
		w.WriteHeader(http.StatusOK)
		for range 1000 {
			if _, err := w.Write([]byte(" ")); err != nil {
				return
			}
			w.(http.Flusher).Flush()
			select {
			case <-r.Context().Done():
				return
			case <-time.After(20 * time.Millisecond):
			}
		}
	}))
	defer server.Close()
	defer server.CloseClientConnections()
	store := openStore(t, server.URL+"/")
	defer store.Close()

	// Progress ends with the job, at its total.
	quick := openStore(t, parity.Store.Mirror).Refresh(parity.Store.Pack)
	var last tg.JobProgress
	for p := range quick.Progress(context.Background()) {
		last = p
	}
	if last.Done != last.Total || !quick.IsEnded() {
		t.Fatal("progress ended at", last)
	}
	quick.Close()

	job := store.Refresh(parity.Store.Pack)
	defer job.Close()
	ctx, cancel := context.WithCancel(context.Background())
	result := make(chan error, 1)
	go func() {
		_, err := job.Wait(ctx)
		result <- err
	}()
	moves := 0
	for range job.Progress(context.Background()) {
		if moves++; moves == 3 {
			break
		}
	}
	if moves != 3 {
		t.Fatal("progress moved", moves, "times")
	}
	at := time.Now()
	cancel()
	select {
	case err := <-result:
		wantCode(t, err, tg.CodeCancelled)
	case <-time.After(5 * time.Second):
		t.Fatal("the cancel did not end the job")
	}
	if time.Since(at) > 2*time.Second || !job.IsEnded() {
		t.Fatal("cancel took", time.Since(at))
	}
}

// A malformed blob is an error, never a panic or a read outside it.
func FuzzReader(f *testing.F) {
	blob, err := tg.SampleRows(5)
	if err != nil {
		f.Fatal(err)
	}
	f.Add(blob)
	f.Add(blob[:40])
	f.Add([]byte("TGB1"))
	f.Fuzz(func(t *testing.T, data []byte) {
		r, err := tg.NewReader(data)
		if err != nil {
			return
		}
		for _, name := range r.Columns() {
			_, _ = r.Strings(name)
			_, _ = r.Nulls(name)
			_, _ = tg.Values[float64](r, name)
			_, _ = tg.Values[uint8](r, name)
			_, _ = r.Column(name)
		}
		_, _ = r.Records()
	})
}

func TestRecords(t *testing.T) {
	p := parity.Engine
	engine := must(tg.NewEngine())
	defer engine.Close()
	must(engine.OpenFile(fixture(p.Fixture)))
	nulls := 0
	for _, blob := range [][]byte{
		must(tg.SampleRows(parity.SampleRows.Rows)),
		must(engine.Ancestors(p.Ancestors.ID)),
		must(engine.Reverse(p.Reverse.Lat, p.Reverse.Lon, p.Reverse.Limit)),
	} {
		r := must(tg.NewReader(blob))
		records := must(r.Records())
		if len(records) != r.Rows() {
			t.Fatalf("%d records, want %d", len(records), r.Rows())
		}
		for _, name := range r.Columns() {
			if !r.Has(name) {
				t.Fatalf("Has(%q) is false for a column the blob has", name)
			}
			isNull := must(r.Nulls(name))
			typ, _ := r.Type(name)
			var want []any
			if typ == tg.TypeUtf8 || typ == tg.TypeDict16 || typ == tg.TypeDict32 {
				for _, v := range must(r.Strings(name)) {
					want = append(want, v)
				}
			} else {
				for _, v := range numbers(t, r, name) {
					want = append(want, v)
				}
			}
			for i, rec := range records {
				got := rec[name]
				switch {
				case isNull != nil && isNull[i]:
					nulls++
					if got != nil {
						t.Fatalf("%s row %d: null read as %v", name, i, got)
					}
				case got == nil:
					t.Fatalf("%s row %d: read as null", name, i)
				case fmt.Sprint(toFloat(got)) != fmt.Sprint(want[i]):
					t.Fatalf("%s row %d: %v, want %v", name, i, got, want[i])
				}
			}
		}
	}
	if nulls == 0 {
		t.Fatal("no null value was checked")
	}
	r := must(tg.NewReader(must(tg.SampleRows(1))))
	if r.Has("nope") || r.Has("") {
		t.Fatal("Has is true for a column the blob does not have")
	}
	if _, err := r.Column("nope"); err == nil {
		t.Fatal("a missing column read")
	}
}

// toFloat widens a number to float64, as numbers does, and leaves a string.
func toFloat(v any) any {
	switch n := v.(type) {
	case int32:
		return float64(n)
	case uint32:
		return float64(n)
	case int64:
		return float64(n)
	case uint64:
		return float64(n)
	case float32:
		return float64(n)
	case uint8:
		return float64(n)
	case int16:
		return float64(n)
	case uint16:
		return float64(n)
	}
	return v
}
