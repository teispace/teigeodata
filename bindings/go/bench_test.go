package teigeodata_test

// The binding benchmark's Go rows (docs/02-research/benches/s4-bindings.md), the same four
// cases as every language: bindings/bench.sh runs them with cgo and with purego.
//
//	TEIGEODATA_LIB=../../target/release/libteigeodata_ffi.dylib go test -run '^$' -bench .

import (
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
