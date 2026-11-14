// The parity checks every platform of the Dart binding runs: the native suite
// (parity_test.dart) and the web suite (web_parity_test.dart). Only how a pack is opened
// differs: natively from its path, on the web from its bytes.
import 'dart:convert';
import 'dart:typed_data';

import 'package:teigeodata/teigeodata.dart';
import 'package:test/test.dart';

/// A fixed-width column of a v1 blob, viewed in place; a 64-bit integer column is read from its
/// two halves (exact to 2^53), since dart2js has no `Int64List`.
List<num> column(Uint8List blob, String name) {
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
      1 => blob.buffer.asInt32List(blob.offsetInBytes + at, len ~/ 4),
      3 => [
        for (var r = at; r < at + len; r += 8)
          view.getInt32(r + 4, Endian.little) * 0x100000000 + view.getUint32(r, Endian.little),
      ],
      5 => blob.buffer.asFloat32List(blob.offsetInBytes + at, len ~/ 4),
      6 => blob.buffer.asFloat64List(blob.offsetInBytes + at, len ~/ 8),
      8 => blob.buffer.asUint8List(blob.offsetInBytes + at, len),
      9 => blob.buffer.asInt16List(blob.offsetInBytes + at, len ~/ 2),
      10 => blob.buffer.asUint16List(blob.offsetInBytes + at, len ~/ 2),
      _ => blob.buffer.asUint32List(blob.offsetInBytes + at, len ~/ 4),
    };
  }
  throw StateError('no column $name');
}

/// The column names of a v1 blob, in order.
List<String> columnNames(Uint8List blob) => [
  for (var i = 0; i < ByteData.sublistView(blob).getUint32(8, Endian.little); i++)
    utf8.decode(blob.sublist(16 + 40 * i, 32 + 40 * i).takeWhile((b) => b != 0).toList()),
];

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

/// Opens the fixture pack `rel` (relative to `bindings/`) in `engine`, with `options` if given;
/// returns its layer ID.
typedef Opener = int Function(Engine engine, String rel, [OpenOptions? options]);

/// Declares the shared checks. `lib` gives the instance the suite made (after its setup);
/// `file` gives a fixture's bytes.
void checks(
  Map<String, dynamic> parity,
  TeiGeodata Function() lib,
  Uint8List Function(String rel) file,
  Opener open,
) {
  test('version and noop', () {
    final tg = lib();
    expect(tg.version(), parity['version']);
    expect(tg.components(), parity['components']);
    tg.noop();
  });

  test('fold', () {
    final tg = lib();
    for (final pair in parity['fold'] as List) {
      expect(tg.fold(pair[0] as String), pair[1]);
    }
  });

  test('sampleRows', () {
    final tg = lib();
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
    expect(columnNames(blob), s['columns']);
    expect(strings(blob, 'name').last, s['last_name']);
    expect(TeiGeodata.sampleRowsMax, parity['sample_rows_max']);
  });

  test('distanceM', () {
    final tg = lib();
    final d = parity['distance_m'] as Map<String, dynamic>;
    final a = (d['args'] as List).cast<num>().map((v) => v.toDouble()).toList();
    expect(tg.distanceM(a[0], a[1], a[2], a[3]), closeTo(d['metres'] as double, 1e-6));
    final b = (d['out_of_range'] as List).cast<num>().map((v) => v.toDouble()).toList();
    expect(() => tg.distanceM(b[0], b[1], b[2], b[3]), throwsA(isA<InvalidArgumentException>()));
  });

  test('errors carry their code', () {
    final tg = lib();
    // The stable codes, the same in every language.
    final codes = <String, int>{
      'InvalidArgument': const InvalidArgumentException('').code,
      'Cancelled': const CancelledException('').code,
      'Internal': const InternalException('').code,
      'Io': const IoException('').code,
      'Corrupt': const CorruptException('').code,
      'MissingComponent': const MissingComponentException('').code,
      'Network': const NetworkException('').code,
      'Refused': const RefusedException('').code,
    };
    expect(codes, parity['error_codes']);
    expect(
      () => tg.sampleRows(TeiGeodata.sampleRowsMax + 1),
      throwsA(
        isA<InvalidArgumentException>().having((e) => e.message, 'message', contains('at most')),
      ),
    );
  });

  test('progress calls back, cancels and rethrows', () {
    final tg = lib();
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
    final tg = lib();
    final e = parity['engine'] as Map<String, dynamic>;
    final engine = Engine(tg);
    expect(open(engine, e['fixture'] as String), 0);
    final g = e['get'] as Map<String, dynamic>;
    final row = engine.get(g['id'] as String);
    expect(strings(row, 'name'), [g['name']]);
    expect(strings(row, 'kind'), [g['kind']]);
    expect(columnNames(row), e['columns']);
    expect((column(row, 'lat') as Float64List).single, g['lat']);
    expect((column(row, 'lon') as Float64List).single, g['lon']);
    final q = e['search'] as Map<String, dynamic>;
    expect(
      strings(engine.search(q['query'] as String, q['limit'] as int), 'id').first,
      q['first_id'],
    );
    final r = e['reverse'] as Map<String, dynamic>;
    final near = engine.reverse(r['lat'] as double, r['lon'] as double, r['limit'] as int);
    expect(strings(near, 'id'), r['ids']);
    expect(
      (column(near, 'meters') as Float64List).first,
      closeTo(r['first_meters'] as double, 0.1),
    );
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
    expect(
      strings(engine.withinRadius(wl, wo, w['meters'] as double, w['limit'] as int), 'id'),
      w['ids'],
    );
    final tz = e['tz_at'] as Map<String, dynamic>;
    expect(strings(engine.tzAt(tz['lat'] as double, tz['lon'] as double), 'tz'), [tz['tz']]);
    final iso = e['by_iso'] as Map<String, dynamic>;
    expect(strings(engine.byIso(iso['code'] as String), 'id'), [iso['id']]);
    expect(() => engine.byIso(e['bad_iso'] as String), throwsA(isA<InvalidArgumentException>()));
    expect(() => engine.withinRadius(wl, wo, -1, 1), throwsA(isA<InvalidArgumentException>()));
    final sw = e['search_with'] as Map<String, dynamic>;
    final focus = SearchOptions(
      focusLat: sw['focus_lat'] as double,
      focusLon: sw['focus_lon'] as double,
      focusScaleM: sw['focus_scale_m'] as double,
    );
    expect(
      strings(engine.searchWith(sw['query'] as String, sw['limit'] as int, focus), 'id'),
      sw['ids'],
    );
    expect(
      strings(
        engine.searchWith(sw['query'] as String, sw['limit'] as int, const SearchOptions()),
        'id',
      ),
      isNot(sw['ids']),
    );
    expect(
      () => engine.searchWith('x', 1, const SearchOptions(focusLat: 1)),
      throwsA(isA<InvalidArgumentException>()),
    );
    final ow = e['open_with'] as Map<String, dynamic>;
    final fixture = e['fixture'] as String;
    final bad = Uint8List.fromList(file(fixture));
    bad[ow['corrupt_at'] as int] ^= 0xff;
    final scratch = Engine(tg);
    expect(
      () => scratch.openBytesWith(bad, const OpenOptions(verify: Verify.eager)),
      throwsA(isA<CorruptException>()),
    );
    expect(scratch.openBytesWith(bad, const OpenOptions()), 0);
    const every = OpenOptions(priority: 2, verify: Verify.eager, load: Load.read, outlineLevel: 1);
    expect(open(scratch, fixture, every), 1);
    expect(
      () => open(scratch, fixture, OpenOptions(outlineLevel: ow['bad_outline_level'] as int)),
      throwsA(isA<InvalidArgumentException>()),
    );
    scratch.close();
    final points = Engine(tg);
    open(
      points,
      fixture,
      OpenOptions(components: ow['components'] as String, languages: ow['languages'] as String),
    );
    expect(strings(points.get((e['get'] as Map<String, dynamic>)['id'] as String), 'id').length, 1);
    expect(() => points.search('Synthland', 1), throwsA(isA<MissingComponentException>()));
    expect(
      () => open(points, fixture, OpenOptions(components: ow['bad_components'] as String)),
      throwsA(isA<InvalidArgumentException>()),
    );
    points.close();
    final pl = e['postal_lookup'] as Map<String, dynamic>;
    final hit = engine.postalLookup(pl['code'] as String, pl['scheme'] as String);
    expect([strings(hit, 'postal'), strings(hit, 'id')], [pl['postal'], pl['ids']]);
    final pp = e['postal_prefix'] as Map<String, dynamic>;
    expect(
      strings(
        engine.postalPrefix(pp['input'] as String, pp['scheme'] as String, pp['limit'] as int),
        'postal',
      ),
      pp['postal'],
    );
    final pa = e['postal_at'] as Map<String, dynamic>;
    final at = engine.postalAt(pa['lat'] as double, pa['lon'] as double);
    expect([strings(at, 'postal'), strings(at, 'code_kind')], [pa['postal'], pa['code_kind']]);
    final po = e['postal_codes_of'] as Map<String, dynamic>;
    expect(strings(engine.postalCodesOf(po['id'] as String), 'postal'), po['postal']);
    expect(() => engine.get(e['bad_id'] as String), throwsA(isA<InvalidArgumentException>()));
    expect(
      () => engine.openBytes(Uint8List.fromList(utf8.encode('not a pack'))),
      throwsA(isA<CorruptException>()),
    );
    expect(engine.layers(), 1);
    final h = e['elevation_at'] as Map<String, dynamic>;
    expect(open(engine, h['fixture'] as String), 1);
    final height = engine.elevationAt(h['lat'] as double, h['lon'] as double);
    expect((column(height, 'meters') as Float64List).single, h['meters']);
    expect(strings(height, 'source'), [h['source']]);
    expect([engine.remove(1), engine.remove(1), engine.layers()], [true, false, 1]);
    engine.close();
    engine.close(); // idempotent
    expect(engine.layers, throwsStateError);
    // An engine never closed is freed by its finalizer.
    Engine(tg).openBytes(file(fixture));
  });

  test('zone rules', () {
    final tg = lib();
    final p = parity['tz'] as Map<String, dynamic>;
    final engine = Engine(tg);
    addTearDown(engine.close);
    open(engine, p['fixture'] as String);
    expect(engine.tzVersion(), p['version']);
    expect(strings(engine.tzZones(), 'zone'), p['zones']);
    final o = p['offset'] as Map<String, dynamic>;
    final got = engine.tzOffsetAt(o['zone'] as String, o['unix'] as int);
    expect(column(got, 'seconds'), [o['seconds']]);
    expect(column(got, 'dst'), [o['dst']]);
    expect(strings(got, 'abbr'), [o['abbr']]);
    for (final kind in ['gap', 'overlap']) {
      final c = p[kind] as Map<String, dynamic>;
      final v = (c['civil'] as List).cast<int>();
      final l = engine.tzLocal(c['zone'] as String, v[0], v[1], v[2], v[3], v[4], v[5]);
      expect(strings(l, 'kind'), [kind]);
      expect(
        [...column(l, 'before') as List<int>, ...column(l, 'after') as List<int>],
        [c['before'], c['after']],
      );
    }
    final st = p['start'] as Map<String, dynamic>;
    expect(column(engine.tzRulesStart(st['zone'] as String), 'unix'), [st['unix']]);
    expect(strings(engine.tzOffsetAt(p['unknown'] as String, 0), 'abbr'), isEmpty);
    expect(
      () => engine.tzLocal(o['zone'] as String, 2024, 13, 1, 0, 0, 0),
      throwsA(isA<InvalidArgumentException>()),
    );
  });
}
