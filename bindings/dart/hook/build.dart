// The build hook (D-289): gives Dart and Flutter the native library `teigeodata` for the
// target being built, as the code asset behind `package:teigeodata/src/bindings.g.dart`.
//
// Where the library comes from, first match wins:
//
// 1. `hooks.user_defines.teigeodata.library`: a library file already built (any variant).
// 2. `hooks.user_defines.teigeodata.source`, or this package inside the teigeodata repository:
//    built from source with cargo (`crates/teigeodata-c`), Rust from `rust-toolchain.toml`.
// 3. Otherwise, or with `prebuilt: true`, the release's prebuilt library for the target,
//    checked against its pinned SHA-256 (hook/src/prebuilt_hashes.dart).
import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:hooks/hooks.dart';

import 'src/prebuilt.dart';
import 'src/target.dart';

void main(List<String> args) async {
  await build(args, (input, output) async {
    if (!input.config.buildCodeAssets) return;
    final code = input.config.code;
    final target = Target.of(code);

    final File library;
    final given = input.userDefines.path('library');
    // `prebuilt: true` uses the release's library even inside the repository (to check one).
    final prebuilt = input.userDefines['prebuilt'] == true;
    final source = prebuilt
        ? null
        : input.userDefines.path('source') ?? _repository(input.packageRoot);
    if (given != null) {
      library = File.fromUri(given);
      if (!library.existsSync()) {
        throw ArgumentError(
          'teigeodata: hooks.user_defines.teigeodata.library: no file at '
          '${library.path}',
        );
      }
      output.dependencies.add(library.uri);
    } else if (source != null) {
      library = await _cargo(input, output, target, source);
    } else {
      library = await fetchPrebuilt(input, target);
    }

    output.assets.code.add(
      CodeAsset(
        package: input.packageName,
        name: 'src/bindings.g.dart',
        linkMode: DynamicLoadingBundled(),
        file: library.absolute.uri,
      ),
    );
  });
}

/// The repository's root when this package is `bindings/dart` inside it.
Uri? _repository(Uri package) {
  final root = package.resolve('../../');
  return File.fromUri(root.resolve('crates/teigeodata-c/Cargo.toml')).existsSync() ? root : null;
}

/// `cargo build --release -p teigeodata-c` for [target] in the repository's own `target/`, so
/// the hook shares its build cache with every other build of the repository.
Future<File> _cargo(BuildInput input, BuildOutputBuilder output, Target target, Uri root) async {
  final dir = Directory.fromUri(root);
  final host = target.triple == hostTriple();
  final args = [
    'build',
    '--release',
    '--locked',
    '-p',
    'teigeodata-c',
    '--lib',
    if (!host) ...['--target', target.triple],
  ];
  final result = await Process.run(
    'cargo',
    args,
    workingDirectory: dir.path,
    environment: target.cargoEnvironment(input.config.code),
    runInShell: Platform.isWindows,
  );
  if (result.exitCode != 0) {
    final err = '${result.stderr}';
    final hint = err.contains('target may not be installed')
        ? 'Add the target first: rustup target add ${target.triple}\n'
        : '';
    throw ProcessException('cargo', args, '$hint${result.stdout}\n$err', result.exitCode);
  }
  final out = Directory.fromUri(
    root.resolve(host ? 'target/release/' : 'target/${target.triple}/release/'),
  );
  final library = File('${out.path}${target.fileName}');
  // Rebuilt when any source cargo read changes: its dependency file lists them all.
  final deps = File('${out.path}${target.stem}.d');
  if (deps.existsSync()) {
    output.dependencies.addAll(cargoDependencies(deps.readAsStringSync()));
  }
  output.dependencies.add(root.resolve('Cargo.lock'));
  return library;
}

/// The files a cargo `.d` file names after its first colon (spaces in paths are escaped).
Iterable<Uri> cargoDependencies(String text) sync* {
  final colon = text.indexOf(': ');
  if (colon < 0) return;
  final body = text.substring(colon + 2).replaceAll('\\\n', ' ');
  for (final m in RegExp(r'((?:\\ |[^\s])+)').allMatches(body)) {
    yield Uri.file(m.group(1)!.replaceAll(r'\ ', ' '));
  }
}

/// The host's Rust triple, as `rustc -vV` prints it.
String hostTriple() {
  final out = Process.runSync('rustc', ['-vV'], runInShell: Platform.isWindows).stdout as String;
  return RegExp(r'^host: (\S+)$', multiLine: true).firstMatch(out)!.group(1)!;
}
