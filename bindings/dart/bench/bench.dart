// The binding benchmark's Dart rows (docs/02-research/benches/s4-bindings.md).
// Run: dart build cli -t bench/bench.dart -o ../../target/dart-bench && ../../target/dart-bench/bundle/bin/bench
// With TEIGEODATA_BENCH_PACK naming a pack, the fifth case runs the 1,000-row query on it.
import 'dart:convert';
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

/// The 1,000-row query's decoding: every `name` as a Dart `String`, and the sum of `lat`.
double decode(Uint8List blob) {
  final v = ByteData.sublistView(blob);
  int u32(int at) => v.getUint32(at, Endian.little);
  int descriptor(String name) {
    for (var i = 0; i < u32(8); i++) {
      final d = 16 + 40 * i;
      if (ascii.decode(blob.sublist(d, d + 16).takeWhile((b) => b != 0).toList()) == name) {
        return d + 16;
      }
    }
    throw StateError('no column $name');
  }

  final rows = u32(4), names = descriptor('name'), lat = descriptor('lat');
  final offsets = u32(names + 4), text = u32(names + 12);
  var total = 0.0;
  for (var r = 0; r < rows; r++) {
    final at = text + u32(offsets + 4 * r), end = text + u32(offsets + 4 * r + 4);
    total += utf8.decoder.convert(blob, at, end).length;
  }
  for (final x in blob.buffer.asFloat64List(blob.offsetInBytes + u32(lat + 4), rows)) {
    total += x;
  }
  return total;
}

void main() {
  final tg = TeiGeodata();
  var sink = 0.0;
  void row(String c, double ns) =>
      print('${'dart'.padRight(10)} ${c.padRight(22)} ${ns.toStringAsFixed(1).padLeft(10)} ns');
  row('noop', best(10000000, tg.noop));
  row('fold', best(1000000, () => sink += tg.fold('Kathmandu').length));
  row('sample_rows(1000)+sum', best(10000, () => sink += latSum(tg.sampleRows(1000))));
  row('callback', best(1000, () => sink += tg.progress(1000, (_) => true)) / 1000);
  final pack = Platform.environment['TEIGEODATA_BENCH_PACK'];
  if (pack != null) {
    final engine = Engine(tg)..openFile(pack);
    Uint8List query() => engine.withinRadius(27.7, 85.3, 50000, 1000);
    if (ByteData.sublistView(query()).getUint32(4, Endian.little) == 1000) {
      row('radius(1000)+decode', best(200, () => sink += decode(query())));
    } else {
      print('dart: $pack: the query does not return 1,000 rows');
    }
    engine.close();
  }
  if (sink == 42) print('');
}
