#!/bin/sh
# The binding benchmark (docs/02-research/benches/s4-bindings.md): builds every artifact
# explicitly (a stale cdylib once produced wrong numbers), checks parity, then runs the same
# four cases in each language. Usage: bindings/bench.sh [rounds]   (from the repository root)
set -eu
rounds=${1:-1}
root=$(pwd)
lib=target/release
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
case "$(uname)" in Darwin) ext=dylib ;; *) ext=so ;; esac

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
(cd bindings/dart && dart compile exe bench/bench.dart -o ../../target/dart-bench >/dev/null)

jvm="java --enable-native-access=ALL-UNNAMED -Djava.library.path=target/release -cp jvm/build"
i=0
while [ "$i" -lt "$rounds" ]; do
  i=$((i + 1))
  echo "# round $i, $(date -u +%FT%TZ), load: $(uptime | sed 's/.*load averages*: //')"
  target/release/examples/bench
  target/c-bench
  (cd bindings/swift && .build/release/Bench)
  target/dart-bench
  (cd bindings/node && node bench.js)
  (cd bindings && $jvm org.teispace.teigeodata.Bench)
  (cd bindings/python && PYTHONPATH=. python3 bench.py)
done
