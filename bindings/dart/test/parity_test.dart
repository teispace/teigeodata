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

/// A fixed-width column of a v1 blob, viewed in place.
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

/// A string column (UTF-8 or a dictionary) of a v1 blob, null rows as null.
List<String?> strings(Uint8List blob, String name) {
  final v = ByteData.sublistView(blob);
  int u32(int at) => v.getUint32(at, Endian.little);
  String text(int at, int end) => utf8.decode(blob.sublist(at, end));
  final rows = u32(4);
  for (var i = 0; i < u32(8); i++) {
    final d = 16 + 40 * i;
    if (utf8.decode(blob.sublist(d, d + 16).takeWhile((b) => b != 0).toList()) != name) continue;
    final (type, at, aux, nulls) = (u32(d + 16), u32(d + 20), u32(d + 28), u32(d + 36));
    return [
      for (var r = 0; r < rows; r++)
        if (nulls != 0 && (blob[nulls + (r >> 3)] >> (r & 7)) & 1 == 0)
          null
        else if (type == 7)
          text(aux + u32(at + 4 * r), aux + u32(at + 4 * r + 4))
        else
          (() {
            final code = type == 11 ? v.getUint16(at + 2 * r, Endian.little) : u32(at + 4 * r);
            final base = aux + 8 + 4 * u32(aux);
            return text(base + u32(aux + 4 + 4 * code), base + u32(aux + 8 + 4 * code));
          })(),
    ];
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

  test('engine handle on the fixture pack', () {
    final e = parity['engine'] as Map<String, dynamic>;
    final engine = Engine(tg);
    expect(engine.openFile('../${e['fixture']}'), 1);
    final g = e['get'] as Map<String, dynamic>;
    final row = engine.get(g['id'] as String);
    expect(strings(row, 'name'), [g['name']]);
    expect(strings(row, 'kind'), [g['kind']]);
    expect((column(row, 'lat') as Float64List).single, g['lat']);
    final q = e['search'] as Map<String, dynamic>;
    expect(strings(engine.search(q['query'] as String, q['limit'] as int), 'id').first, q['first_id']);
    final r = e['reverse'] as Map<String, dynamic>;
    final near = engine.reverse(r['lat'] as double, r['lon'] as double, r['limit'] as int);
    expect(strings(near, 'id'), r['ids']);
    expect((column(near, 'meters') as Float64List).first, closeTo(r['first_meters'] as double, 0.1));
    final a = e['admin_at'] as Map<String, dynamic>;
    expect(strings(engine.adminAt(a['lat'] as double, a['lon'] as double), 'id'), a['ids']);
    for (final (method, call) in [
      ('parent', engine.parent),
      ('ancestors', engine.ancestors),
      ('children', engine.children),
    ]) {
      final m = e[method] as Map<String, dynamic>;
      expect(strings(call(m['id'] as String), 'id'), m['ids'], reason: method);
    }
    final nb = e['nearby'] as Map<String, dynamic>;
    expect(strings(engine.nearby(nb['id'] as String, nb['limit'] as int), 'id'), nb['ids']);
    final w = e['within_radius'] as Map<String, dynamic>;
    final (wl, wo) = (w['lat'] as double, w['lon'] as double);
    expect(strings(engine.withinRadius(wl, wo, w['meters'] as double, w['limit'] as int), 'id'), w['ids']);
    final tz = e['tz_at'] as Map<String, dynamic>;
    expect(strings(engine.tzAt(tz['lat'] as double, tz['lon'] as double), 'tz'), [tz['tz']]);
    final iso = e['by_iso'] as Map<String, dynamic>;
    expect(strings(engine.byIso(iso['code'] as String), 'id'), [iso['id']]);
    expect(() => engine.byIso(e['bad_iso'] as String), throwsA(isA<InvalidArgumentException>()));
    expect(() => engine.withinRadius(wl, wo, -1, 1), throwsA(isA<InvalidArgumentException>()));
    expect(() => engine.get(e['bad_id'] as String), throwsA(isA<InvalidArgumentException>()));
    expect(() => engine.openFile('/nonexistent.tgd'), throwsA(isA<IoException>()));
    expect(() => engine.openBytes(Uint8List.fromList(utf8.encode('not a pack'))), throwsA(isA<CorruptException>()));
    expect(engine.layers(), 1);
    engine.close();
    engine.close(); // idempotent
    expect(engine.layers, throwsStateError);
    // An engine never closed is freed by its finalizer.
    Engine(tg).openBytes(File('../${e['fixture']}').readAsBytesSync());
  });
}
