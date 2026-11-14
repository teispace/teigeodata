#!/bin/sh
# The binding benchmark (docs/02-research/benches/s4-bindings.md): builds every artifact
# explicitly (a stale cdylib once produced wrong numbers), checks parity, then runs the same
# cases in each language. Usage: bindings/bench.sh [rounds]   (from the repository root)
# The fifth case, a 1,000-row query decoded in each language, runs on the pack that
# TEIGEODATA_BENCH_PACK names (default: the Nepal pack of a local data build, if present).
set -eu
rounds=${1:-1}
root=$(pwd)
lib=target/release
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
case "$(uname)" in Darwin) ext=dylib ;; *) ext=so ;; esac
pack=${TEIGEODATA_BENCH_PACK:-data/build/pack/np/np-places-lite.tgd}
if [ -f "$pack" ]; then
  TEIGEODATA_BENCH_PACK=$(cd "$(dirname "$pack")" && pwd)/$(basename "$pack")
  export TEIGEODATA_BENCH_PACK
else
  echo "# no pack at $pack: the 1,000-row query is skipped"
  unset TEIGEODATA_BENCH_PACK
fi

cargo build -q --release -p teigeodata-ffi --lib --example bench
cc -O2 -std=gnu11 bindings/c/bench.c -Ibindings/c -L$lib -lteigeodata_ffi \
  -Wl,-rpath,"$root/$lib" -o target/c-bench
(
  cd bindings
  PYO3_BUILD_EXTENSION_MODULE=1 cargo build -q --release -p teigeodata-python
  cargo build -q --release -p teigeodata-node -p teigeodata-jvm
  cp target/release/libteigeodata_python.$ext python/teigeodata/_native.abi3.so
  cp target/release/libteigeodata_node.$ext node/teigeodata.node
  rm -rf jvm/build
  javac -nowarn -d jvm/build jvm/java/org/teispace/teigeodata/*.java \
    jvm/test/org/teispace/teigeodata/*.java 2>/dev/null
)
(cd bindings/swift && swift build -q -c release)
# `dart build cli` runs the build hook and bundles the library; `compile exe` does not.
(cd bindings/dart && dart build cli -t bench/bench.dart -o ../../target/dart-bench >/dev/null)

jvm="java --enable-native-access=ALL-UNNAMED -Djava.library.path=target/release -cp jvm/build"
i=0
while [ "$i" -lt "$rounds" ]; do
  i=$((i + 1))
  echo "# round $i, $(date -u +%FT%TZ), load: $(uptime | sed 's/.*load averages*: //')"
  target/release/examples/bench
  target/c-bench
  (cd bindings/swift && .build/release/Bench)
  target/dart-bench/bundle/bin/bench
  (cd bindings/node && node bench.js)
  (cd bindings && $jvm org.teispace.teigeodata.Bench)
  (cd bindings/python && PYTHONPATH=. python3 bench.py)
  for cgo in 1 0; do
    (cd bindings/go && CGO_ENABLED=$cgo TEIGEODATA_LIB="$root/$lib/libteigeodata_ffi.$ext" \
      go test -run '^$' -bench . -count 1 . | awk -v name="go-cgo$cgo" '
        /^BenchmarkNoop/ { c = "noop"; v = $3 }
        /^BenchmarkFold/ { c = "fold"; v = $3 }
        /^BenchmarkSampleRowsSum/ { c = "sample_rows(1000)+sum"; v = $3 }
        /^BenchmarkCallback/ { c = "callback"; v = $5 }
        /^BenchmarkRadius/ { c = "radius(1000)+decode"; v = $3 }
        /^Benchmark/ { printf "%-10s %-22s %10.1f ns\n", name, c, v }')
  done
done
