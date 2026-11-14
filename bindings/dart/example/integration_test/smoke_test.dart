// The library reaches the app on this platform through the build hook, and its calls work:
// synchronous ones, a callback, and one on a helper isolate.
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:teigeodata/teigeodata.dart';
import 'package:teigeodata_example/main.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  final tg = TeiGeodata();

  test('calls', () {
    expect(tg.version(), matches(RegExp(r'^\d+\.\d+\.\d+')));
    expect(tg.fold('Kathmandu'), 'kathmandu');
    final blob = tg.sampleRows(1000);
    expect(blob.sublist(0, 4), Uint8List.fromList('TGB1'.codeUnits));
    expect(tg.distanceM(27.7, 85.3, 27.7, 85.3), 0);
    final seen = <int>[];
    expect(tg.progress(3, (step) => (seen..add(step)).isNotEmpty), 3);
    expect(seen, [1, 2, 3]);
    expect(() => tg.sampleRows(-1), throwsA(isA<InvalidArgumentException>()));
  });

  test('a helper isolate resolves the same library', () async {
    final missing = '${Directory.systemTemp.createTempSync('tg-').path}/missing';
    await expectLater(Store.openAsync(tg, missing), throwsA(isA<RefusedException>()));
  });

  testWidgets('the app shows the version', (tester) async {
    await tester.pumpWidget(const App());
    expect(find.textContaining('teigeodata ${tg.version()}: lahachowk'), findsOneWidget);
  });
}
