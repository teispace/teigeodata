// The tour: every query of the guide, run on a released pack.
//
// Usage: dart run bin/tour.dart world-np-lite.tgd

// #region import
import 'package:teigeodata/teigeodata.dart';

// #endregion import

void main(List<String> args) async {
  final pack = args[0];

  print('=== open');
  // #region open
  await TeiGeodata.init(); // loads the module on the web; returns at once natively
  final tg = TeiGeodata();
  final engine = Engine(tg);
  engine.openFile(pack);
  print('layers: ${engine.layers()}');
  // #endregion open

  print('=== search');
  // #region search
  for (final p in Reader(engine.search('Pokhara', 3))) {
    final (lat, lon) = (p['lat'] as double, p['lon'] as double);
    print(
      '${p['id']}  ${p['name']}  ${p['kind']}  ${lat.toStringAsFixed(4)}, '
      '${lon.toStringAsFixed(4)}  score ${p['score']}',
    );
  }
  // #endregion search

  print('=== search-parent');
  // #region search-parent
  for (final p in Reader(engine.search('Kathmandu, Bagmati', 2))) {
    print('${p['id']}  ${p['name']}  ${p['kind']} ${p['level']}');
  }
  // #endregion search-parent

  print('=== search-focus');
  // #region search-focus
  // Many places share a name; a focus (the user's location) ranks the near ones first.
  const options = SearchOptions(
    focusLat: 28.7,
    focusLon: 80.6,
    focusScaleM: 30000,
  );
  for (final p in Reader(engine.searchWith('Shivapur', 3, options))) {
    final (lat, lon) = (p['lat'] as double, p['lon'] as double);
    print(
      '${p['id']}  ${p['name']}  ${lat.toStringAsFixed(4)}, ${lon.toStringAsFixed(4)}',
    );
  }
  // #endregion search-focus

  print('=== get');
  // #region get
  final place = Reader(engine.get('tgd:np/1xhb')).single;
  print('${place['name']} ${place['kind']}');
  final (lat, lon) = (place['lat'] as double, place['lon'] as double);
  print(
    'at ${lat.toStringAsFixed(6)}, ${lon.toStringAsFixed(6)}, ${place['elevation']} m',
  );
  print('timezone ${place['tz']} parent ${place['parent']}');
  // #endregion get

  print('=== hierarchy');
  // #region hierarchy
  for (final p in Reader(engine.ancestors('tgd:np/1xhb'))) {
    print(
      '${p['kind']} ${p['level']}  ${p['name']}  ${p['iso'] ?? ''}'.trimRight(),
    );
  }
  final children = Reader(engine.children('tgd:np/295c'));
  print('Kaski has ${children.rows} children');
  // #endregion hierarchy

  print('=== reverse');
  // #region reverse
  for (final p in Reader(engine.reverse(27.7172, 85.3240, 3))) {
    final (meters, bearing) = (p['meters'] as double, p['bearing'] as double);
    print(
      '${p['name']}  ${meters.toStringAsFixed(0)} m at ${bearing.toStringAsFixed(0)}°',
    );
  }
  // #endregion reverse

  print('=== radius');
  // #region radius
  final within = Reader(
    engine.withinRadius(27.7172, 85.3240, 500, 10),
  ).records();
  final farthest = within.last['meters'] as double;
  print(
    '${within.length} places within 500 m; farthest ${farthest.toStringAsFixed(0)} m',
  );
  // #endregion radius

  print('=== nearby');
  // #region nearby
  for (final p in Reader(engine.nearby('tgd:np/2cn', 3))) {
    print('${p['name']}  ${(p['meters'] as double).toStringAsFixed(0)} m');
  }
  // #endregion nearby

  print('=== admin');
  // #region admin
  for (final p in Reader(engine.adminAt(28.2096, 83.9856))) {
    print('${p['level']}  ${p['name']}  ${p['iso'] ?? ''}'.trimRight());
  }
  final state = Reader(engine.byIso('NP-P4')).single;
  print('NP-P4 is ${state['name']} ${state['id']}');
  // #endregion admin

  print('=== timezone');
  // #region timezone
  final zone = Reader(engine.tzAt(27.7172, 85.3240)).single['tz'] as String;
  final offset = Reader(engine.tzOffsetAt(zone, 1700000000)).single;
  print(
    '$zone ${offset['seconds']} s east of UTC, ${offset['abbr']} dst ${offset['dst']}',
  );
  final local = Reader(
    engine.tzLocal('America/New_York', 2026, 3, 8, 2, 30, 0),
  ).single;
  print(
    '02:30 on 2026-03-08 in New York: ${local['kind']} ${local['before']} -> ${local['after']}',
  );
  print('tzdb ${engine.tzVersion()}');
  // #endregion timezone

  print('=== elevation');
  // #region elevation
  final height = Reader(engine.elevationAt(27.7172, 85.3240)).single;
  final (meters, grid) = (
    height['meters'] as double,
    height['resolution_m'] as double,
  );
  print(
    '${meters.toStringAsFixed(1)} m above ${height['datum']} '
    '(${height['source']}, ${grid.toStringAsFixed(0)} m grid)',
  );
  // #endregion elevation

  print('=== distance');
  // #region distance
  final km = tg.distanceM(27.7172, 85.3240, 28.2096, 83.9856) / 1000;
  print('Kathmandu to Pokhara: ${km.toStringAsFixed(1)} km');
  // #endregion distance

  print('=== errors');
  // #region errors
  try {
    engine.reverse(91, 0, 1);
  } on InvalidArgumentException catch (error) {
    print('invalid argument: ${error.message}');
  }
  // #endregion errors

  engine.close();
  tg.close();
}
