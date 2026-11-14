// Parity: the Dart binding gives the results bindings/parity.json expects (checked there
// against Rust). `dart test` builds the library through the package's build hook. The checks
// shared with the web suite are in src/checks.dart; these add what needs a file system.
@TestOn('vm')
library;

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:teigeodata/teigeodata.dart';
import 'package:test/test.dart';

import 'src/checks.dart';

void main() {
  // TEIGEODATA_PARITY names another copy (cargo xtask parity-mutants plants a difference in one).
  final path = Platform.environment['TEIGEODATA_PARITY'] ?? '../parity.json';
  final parity = jsonDecode(File(path).readAsStringSync()) as Map<String, dynamic>;
  final tg = TeiGeodata();
  tearDownAll(tg.close);

  checks(
    parity,
    () => tg,
    (rel) => File('../$rel').readAsBytesSync(),
    (engine, rel, [options]) =>
        options == null ? engine.openFile('../$rel') : engine.openFileWith('../$rel', options),
  );

  test('file paths: a missing file, a pack inside another file', () {
    final engine = Engine(tg);
    addTearDown(engine.close);
    expect(() => engine.openFile('/nonexistent.tgd'), throwsA(isA<IoException>()));
    final p = parity['tz'] as Map<String, dynamic>;
    // The same pack inside another file, opened from its range (an APK asset).
    final e = p['embedded'] as Map<String, dynamic>;
    final inside = '../${e['fixture']}';
    final (offset, length) = (e['offset'] as int, e['length'] as int);
    final embedded = Engine(tg);
    addTearDown(embedded.close);
    embedded.openFileRange(inside, offset, length);
    expect(embedded.tzVersion(), p['version']);
    expect(
      () => embedded.openFileRange(inside, offset, e['past_end'] as int),
      throwsA(isA<IoException>()),
    );
    expect(
      () => embedded.openFileRange(inside, offset - 1, length),
      throwsA(isA<CorruptException>()),
    );
  });

  test('store', () {
    // The store (parity.json, store): the fixture root, a mirror that refuses connections.
    final st = parity['store'] as Map<String, dynamic>;
    final root = File('../${st['root']}').readAsBytesSync();
    final mirror = st['mirror'] as String, pack = st['pack'] as String;
    final d = Directory.systemTemp.createTempSync('tg-store-');
    addTearDown(() => d.deleteSync(recursive: true));
    expect(() => Store.open(tg, '${d.path}/missing'), throwsA(isA<RefusedException>()));
    expect(
      () =>
          Store.create(tg, '${d.path}/bad', mirror, mirror, Uint8List.fromList(utf8.encode('{}'))),
      throwsA(isA<RefusedException>()),
    );
    Store.create(tg, '${d.path}/s', mirror, mirror, root).close();
    final store = Store.open(tg, '${d.path}/s');
    addTearDown(store.close);
    expect(store.damaged(), '');
    expect(strings(store.catalog(), 'path'), isEmpty);
    expect(strings(store.status(false), 'path'), isEmpty);
    final engine = store.engine();
    expect(engine.layers(), 0);
    expect(strings(store.find(engine, pack, 3), 'name'), isEmpty);
    expect(strings(store.locate(engine, 27.7, 85.3, 1000), 'name'), isEmpty);
    expect(() => store.find(engine, pack, 10001), throwsA(isA<InvalidArgumentException>()));
    expect(() => store.locate(engine, 91, 0, 1000), throwsA(isA<InvalidArgumentException>()));
    engine.close();
    expect(
      () => store.install(pack, InstallOptions(components: st['bad_components'] as String)),
      throwsA(isA<InvalidArgumentException>()),
    );
    expect(() => store.remove(pack), throwsA(isA<RefusedException>()));
    expect(
      () => store.installPass(pack, '', InstallOptions()),
      throwsA(isA<InvalidArgumentException>()),
    );
    expect(
      store.installPass(pack, '${d.path}/dl', InstallOptions()).result,
      throwsA(isA<NetworkException>()),
    );
    final job = store.refresh(pack);
    addTearDown(job.close);
    expect(job.result, throwsA(isA<NetworkException>()));
    expect(job.isEnded(), isTrue);
    expect(job.waitProgress(job.seq(), 60000), job.seq());
    expect(job.doneBytes(), job.totalBytes());
    job.cancel();
  });

  test('store async', () async {
    // The asynchronous forms run on a helper isolate and carry the same error codes.
    final st = parity['store'] as Map<String, dynamic>;
    final root = File('../${st['root']}').readAsBytesSync();
    final mirror = st['mirror'] as String, pack = st['pack'] as String;
    final d = Directory.systemTemp.createTempSync('tg-store-');
    addTearDown(() => d.deleteSync(recursive: true));
    await expectLater(Store.openAsync(tg, '${d.path}/missing'), throwsA(isA<RefusedException>()));
    (await Store.createAsync(tg, '${d.path}/s', mirror, mirror, root)).close();
    final store = await Store.openAsync(tg, '${d.path}/s');
    addTearDown(store.close);
    expect(strings(await store.statusAsync(false), 'path'), isEmpty);
    final engine = await store.engineAsync();
    addTearDown(engine.close);
    expect(strings(await store.findAsync(engine, pack, 3), 'name'), isEmpty);
    await expectLater(
      store.findAsync(engine, pack, 10001),
      throwsA(isA<InvalidArgumentException>()),
    );
    final job = store.refresh(pack);
    final result = job.resultAsync();
    // Closed while the call runs: freed when it ends, not under it.
    job.close();
    await expectLater(result, throwsA(isA<NetworkException>()));
    expect(job.isEnded, throwsStateError);
  });

  test('job progress', () async {
    // A mirror in this isolate that trickles a byte every 20 ms: the job runs until cancelled.
    final st = parity['store'] as Map<String, dynamic>;
    final root = File('../${st['root']}').readAsBytesSync();
    final pack = st['pack'] as String;
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    // Ended at teardown: a write to a client that went away need not throw, and a handler
    // still trickling would keep the process alive for its full 20 s.
    var stopped = false;
    server.listen((req) async {
      // Unbuffered: a buffered response holds its body until it closes.
      req.response.bufferOutput = false;
      req.response.contentLength = 1000;
      try {
        for (var i = 0; i < 1000 && !stopped; i++) {
          req.response.add(const [0x20]);
          await req.response.flush();
          await Future<void>.delayed(const Duration(milliseconds: 20));
        }
        await req.response.close();
      } catch (_) {
        // The client went away.
      }
    });
    addTearDown(() {
      stopped = true;
      return server.close(force: true);
    });
    final d = Directory.systemTemp.createTempSync('tg-store-');
    addTearDown(() => d.deleteSync(recursive: true));

    // Progress ends with the job, at its total.
    final mirror = st['mirror'] as String;
    final quick = Store.create(tg, '${d.path}/q', mirror, mirror, root);
    addTearDown(quick.close);
    final last = await quick.refresh(pack).progress().last;
    expect(last.done, last.total);

    final slow = 'http://127.0.0.1:${server.port}/';
    final store = Store.create(tg, '${d.path}/s', slow, slow, root);
    addTearDown(store.close);
    final job = store.refresh(pack);
    addTearDown(job.close);
    final result = job.resultAsync();
    expect(await job.progress().take(3).length, 3, reason: 'progress moves');
    final at = DateTime.now();
    job.cancel();
    await expectLater(result, throwsA(isA<CancelledException>()));
    expect(DateTime.now().difference(at).inSeconds, lessThan(2));
  });
}
