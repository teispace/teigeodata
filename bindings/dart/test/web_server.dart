// The web suite's server (web_parity_test.dart), run on the VM by `spawnHybridUri`: it serves
// the built WebAssembly package (`cargo xtask package-wasm`, or TEIGEODATA_WASM) under /pkg/,
// and sends the page parity.json (TEIGEODATA_PARITY names another copy, as for the native
// suite) and the fixture packs it names.
import 'dart:convert';
import 'dart:io';

import 'package:stream_channel/stream_channel.dart';

const _types = {'.js': 'text/javascript', '.wasm': 'application/wasm', '.json': 'application/json'};

Future<void> hybridMain(StreamChannel<Object?> channel) async {
  final pkg = Directory(
    Platform.environment['TEIGEODATA_WASM'] ?? '../../target/wasm/full',
  ).absolute;
  if (!File('${pkg.path}/index.js').existsSync()) {
    channel.sink.add({'error': 'no WebAssembly package at ${pkg.path}: cargo xtask package-wasm'});
    return;
  }
  final root = pkg.resolveSymbolicLinksSync();
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  server.listen((request) async {
    final response = request.response..headers.set('Access-Control-Allow-Origin', '*');
    final rel = request.uri.path;
    // Only files of the package, nothing above it.
    final file = rel.startsWith('/pkg/') ? File('$root/${rel.substring(5)}') : null;
    if (file == null ||
        !file.existsSync() ||
        !file.resolveSymbolicLinksSync().startsWith('$root${Platform.pathSeparator}')) {
      response.statusCode = HttpStatus.notFound;
      await response.close();
      return;
    }
    final dot = rel.lastIndexOf('.');
    response.headers.set('Content-Type', _types[rel.substring(dot)] ?? 'application/octet-stream');
    await response.addStream(file.openRead());
    await response.close();
  });
  final text = File(
    Platform.environment['TEIGEODATA_PARITY'] ?? '../parity.json',
  ).readAsStringSync();
  final parity = jsonDecode(text) as Map<String, dynamic>;
  final files = {
    for (final rel in fixtures(parity)) rel: base64.encode(File('../$rel').readAsBytesSync()),
  };
  channel.sink.add({'port': server.port, 'parity': text, 'files': files});
  // The server lives until the suite ends: the test runner kills this isolate then.
}

/// Every fixture parity.json names (each `fixture` key, at any depth).
Set<String> fixtures(Object? value) => switch (value) {
  Map<String, dynamic> m => {
    for (final MapEntry(:key, :value) in m.entries)
      ...(key == 'fixture' && value is String ? {value} : fixtures(value)),
  },
  List<dynamic> l => {for (final v in l) ...fixtures(v)},
  _ => {},
};
