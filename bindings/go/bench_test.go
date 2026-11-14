package teigeodata_test

// The binding benchmark's Go rows (docs/02-research/benches/s4-bindings.md), the same four
// cases as every language: bindings/bench.sh runs them with cgo and with purego. With
// TEIGEODATA_BENCH_PACK naming a pack, BenchmarkRadius runs the 1,000-row query on it.
//
//	TEIGEODATA_LIB=../../target/release/libteigeodata_ffi.dylib go test -run '^$' -bench .

import (
	"os"
	"testing"

	tg "github.com/teispace/teigeodata/bindings/go"
)

var sink float64

func BenchmarkNoop(b *testing.B) {
	for b.Loop() {
		_ = tg.Noop()
	}
}

func BenchmarkFold(b *testing.B) {
	for b.Loop() {
		s, _ := tg.Fold("Kathmandu")
		sink += float64(len(s))
	}
}

// BenchmarkSampleRowsSum is sample_rows(1000)+sum: the blob, its reader and the lat column.
func BenchmarkSampleRowsSum(b *testing.B) {
	for b.Loop() {
		blob, _ := tg.SampleRows(1000)
		r, _ := tg.NewReader(blob)
		lat, _ := tg.Values[float64](r, "lat")
		for _, v := range lat {
			sink += v
		}
	}
}

// BenchmarkCallback is one call of progress(1000): ns per callback is ns/op / 1000.
func BenchmarkCallback(b *testing.B) {
	yes := func(uint32) bool { return true }
	for b.Loop() {
		n, _ := tg.Progress(1000, yes)
		sink += float64(n)
	}
	b.ReportMetric(float64(b.Elapsed().Nanoseconds())/float64(b.N)/1000, "ns/callback")
}

// BenchmarkRadius is radius(1000)+decode: the 1,000-row query, every name as a Go string and
// the sum of lat, through the package's Reader.
func BenchmarkRadius(b *testing.B) {
	pack := os.Getenv("TEIGEODATA_BENCH_PACK")
	if pack == "" {
		b.Skip("TEIGEODATA_BENCH_PACK names no pack")
	}
	engine, err := tg.NewEngine()
	if err != nil {
		b.Fatal(err)
	}
	defer engine.Close()
	if _, err := engine.OpenFile(pack); err != nil {
		b.Fatal(err)
	}
	query := func() *tg.Reader {
		blob, err := engine.WithinRadius(27.7, 85.3, 50_000, 1000)
		if err != nil {
			b.Fatal(err)
		}
		r, err := tg.NewReader(blob)
		if err != nil {
			b.Fatal(err)
		}
		return r
	}
	if rows := query().Rows(); rows != 1000 {
		b.Fatalf("%s: the query returns %d rows, not 1,000", pack, rows)
	}
	for b.Loop() {
		r := query()
		names, _ := r.Strings("name")
		for _, n := range names {
			sink += float64(len(n))
		}
		lat, _ := tg.Values[float64](r, "lat")
		for _, v := range lat {
			sink += v
		}
	}
}
