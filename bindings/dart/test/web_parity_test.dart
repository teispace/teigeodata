// Parity on the web: the Dart binding over the WebAssembly module gives the results
// bindings/parity.json expects, compiled by dart2js and by dart2wasm:
//   dart test -p chrome [--compiler dart2js,dart2wasm]
// after `cargo xtask package-wasm` (web_server.dart serves the package). The checks are those
// of the native suite (src/checks.dart); packs open from their bytes.
@TestOn('browser')
library;

import 'dart:convert';
import 'dart:typed_data';

import 'package:teigeodata/teigeodata.dart';
import 'package:test/test.dart';

import 'src/checks.dart';

void main() {
  // Filled before the first test: the checks read them only when they run.
  final parity = <String, dynamic>{};
  final files = <String, Uint8List>{};
  late TeiGeodata tg;

  setUpAll(() async {
    final channel = spawnHybridUri('web_server.dart');
    final setup = (await channel.stream.first as Map).cast<String, Object?>();
    if (setup['error'] case final String error) fail(error);
    // A number crosses the channel as a double under dart2wasm.
    final port = (setup['port']! as num).toInt();
    parity.addAll(jsonDecode(setup['parity']! as String) as Map<String, dynamic>);
    for (final MapEntry(:key, :value) in (setup['files']! as Map).cast<String, String>().entries) {
      files[key] = base64.decode(value);
    }
    // Before the module loads, an instance is refused; a failed load can be tried again.
    expect(TeiGeodata.new, throwsStateError);
    await expectLater(
      TeiGeodata.init(module: Uri.parse('http://127.0.0.1:$port/pkg/missing.js')),
      throwsA(anything),
    );
    await TeiGeodata.init(module: Uri.parse('http://127.0.0.1:$port/pkg/index.js'));
    tg = TeiGeodata();
  });

  checks(
    parity,
    () => tg,
    (rel) => files[rel]!,
    (engine, rel, [options]) => options == null
        ? engine.openBytes(files[rel]!)
        : engine.openBytesWith(files[rel]!, options),
  );
}
