// Parity: the Dart binding gives the results bindings/parity.json expects (checked there
// against Rust). Build the C ABI first: cargo build --release -p teigeodata-ffi.
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:teigeodata/teigeodata.dart';
import 'package:test/test.dart';

String libraryPath() {
  final name = Platform.isMacOS
      ? 'libteigeodata_ffi.dylib'
      : Platform.isWindows
      ? 'teigeodata_ffi.dll'
      : 'libteigeodata_ffi.so';
  return '../../target/release/$name';
}

/// A fixed-width column of a v0 blob, viewed in place.
TypedData column(Uint8List blob, String name) {
  final view = ByteData.sublistView(blob);
  expect(ascii.decode(blob.sublist(0, 4)), 'TGB1');
  for (var i = 0; i < view.getUint32(8, Endian.little); i++) {
    final d = 16 + 40 * i;
    final n = utf8.decode(blob.sublist(d, d + 16).takeWhile((b) => b != 0).toList());
    if (n != name) continue;
    final type = view.getUint32(d + 16, Endian.little);
    final at = view.getUint32(d + 20, Endian.little);
    final len = view.getUint32(d + 24, Endian.little);
    return switch (type) {
      5 => blob.buffer.asFloat32List(blob.offsetInBytes + at, len ~/ 4),
      6 => blob.buffer.asFloat64List(blob.offsetInBytes + at, len ~/ 8),
      _ => blob.buffer.asUint32List(blob.offsetInBytes + at, len ~/ 4),
    };
  }
  throw StateError('no column $name');
}

void main() {
  final parity = jsonDecode(File('../parity.json').readAsStringSync()) as Map<String, dynamic>;
  final tg = TeiGeodata.open(libraryPath());
  tearDownAll(tg.close);

  test('version and noop', () {
    expect(tg.version(), parity['version']);
    tg.noop();
  });

  test('fold', () {
    for (final pair in parity['fold'] as List) {
      expect(tg.fold(pair[0] as String), pair[1]);
    }
  });

  test('sampleRows', () {
    final s = parity['sample_rows'] as Map<String, dynamic>;
    final blob = tg.sampleRows(s['rows'] as int);
    expect(blob.length, s['bytes']);
    var lat = 0.0;
    for (final v in column(blob, 'lat') as Float64List) {
      lat += v;
    }
    expect(lat, s['lat_sum']);
    var elevation = 0.0;
    for (final v in column(blob, 'elevation') as Float32List) {
      elevation += v;
    }
    expect(elevation, s['elevation_sum']);
    expect(TeiGeodata.sampleRowsMax, parity['sample_rows_max']);
  });

  test('distanceM', () {
    final d = parity['distance_m'] as Map<String, dynamic>;
    final a = (d['args'] as List).cast<num>().map((v) => v.toDouble()).toList();
    expect(tg.distanceM(a[0], a[1], a[2], a[3]), closeTo(d['metres'] as double, 1e-6));
    final b = (d['out_of_range'] as List).cast<num>().map((v) => v.toDouble()).toList();
    expect(() => tg.distanceM(b[0], b[1], b[2], b[3]), throwsA(isA<InvalidArgumentException>()));
  });

  test('errors carry their code', () {
    expect(
      () => tg.sampleRows(TeiGeodata.sampleRowsMax + 1),
      throwsA(
        isA<InvalidArgumentException>().having((e) => e.message, 'message', contains('at most')),
      ),
    );
  });

  test('progress calls back, cancels and rethrows', () {
    final seen = <int>[];
    expect(
      tg.progress(3, (s) {
        seen.add(s);
        return true;
      }),
      3,
    );
    expect(seen, [1, 2, 3]);
    final g = parity['progress'] as Map<String, dynamic>;
    expect(
      () => tg.progress(g['steps'] as int, (s) => s < (g['cancel_after'] as int)),
      throwsA(
        isA<CancelledException>().having((e) => e.message, 'message', g['cancelled_message']),
      ),
    );
    final boom = StateError('boom');
    expect(() => tg.progress(5, (_) => throw boom), throwsA(same(boom)));
  });
}
