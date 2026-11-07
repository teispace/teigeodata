// The binding benchmark's Dart rows (docs/02-research/benches/s4-bindings.md).
// Run: dart compile exe bench/bench.dart -o ../../target/dart-bench && ../../target/dart-bench
import 'dart:io';
import 'dart:typed_data';

import 'package:teigeodata/teigeodata.dart';

double best(int n, void Function() f) {
  var out = double.infinity;
  for (var r = 0; r < 5; r++) {
    final sw = Stopwatch()..start();
    for (var i = 0; i < n; i++) {
      f();
    }
    final ns = sw.elapsedMicroseconds * 1000 / n;
    if (ns < out) out = ns;
  }
  return out;
}

double latSum(Uint8List blob) {
  final view = ByteData.sublistView(blob);
  final at = view.getUint32(76, Endian.little), len = view.getUint32(80, Endian.little);
  final lat = blob.buffer.asFloat64List(blob.offsetInBytes + at, len ~/ 8);
  var s = 0.0;
  for (final v in lat) {
    s += v;
  }
  return s;
}

void main() {
  final lib = Platform.isMacOS ? 'libteigeodata_ffi.dylib' : 'libteigeodata_ffi.so';
  final tg = TeiGeodata.open('${File(Platform.resolvedExecutable).parent.path}/release/$lib');
  var sink = 0.0;
  void row(String c, double ns) => print('${'dart'.padRight(10)} ${c.padRight(22)} ${ns.toStringAsFixed(1).padLeft(10)} ns');
  row('noop', best(10000000, tg.noop));
  row('fold', best(1000000, () => sink += tg.fold('Kathmandu').length));
  row('sample_rows(1000)+sum', best(10000, () => sink += latSum(tg.sampleRows(1000))));
  row('callback', best(1000, () => sink += tg.progress(1000, (_) => true)) / 1000);
  if (sink == 42) print('');
}
