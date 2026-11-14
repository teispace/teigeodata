// The release's prebuilt libraries: downloaded once per version and target into the hook's
// shared cache, and used only when their SHA-256 matches the one pinned here.
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:hooks/hooks.dart';

import 'prebuilt_hashes.dart';
import 'target.dart';

/// Where releases are downloaded from: `<base>/v<version>/<asset>`. An organisation can point
/// `hooks.user_defines.teigeodata.prebuilt_base` at a mirror; the hashes still apply.
const _base = 'https://github.com/teispace/teigeodata/releases/download';

/// The release asset holding [target]'s library.
String assetName(String version, Target target) =>
    'teigeodata-$version-${target.triple}-${target.fileName}';

/// The verified prebuilt library for [target], from the cache or downloaded.
Future<File> fetchPrebuilt(BuildInput input, Target target) async {
  final version = packageVersion(input.packageRoot);
  final want = prebuiltSha256[target.triple];
  if (want == null) {
    throw UnsupportedError(
      'teigeodata $version has no prebuilt library for ${target.triple}. Build it from '
      'source: set hooks.user_defines.teigeodata.source to a checkout of the repository.',
    );
  }
  final dir = Directory.fromUri(
    input.outputDirectoryShared.resolve('teigeodata-$version/${target.triple}/'),
  );
  final file = File('${dir.path}${target.fileName}');
  if (file.existsSync() && await _sha256(file) == want) return file;

  final base = (input.userDefines['prebuilt_base'] as String?) ?? _base;
  final url = Uri.parse('$base/v$version/${assetName(version, target)}');
  dir.createSync(recursive: true);
  final part = File('${file.path}.$pid.part');
  final client = HttpClient()..findProxy = HttpClient.findProxyFromEnvironment;
  try {
    final response = await (await client.getUrl(url)).close();
    if (response.statusCode != HttpStatus.ok) {
      throw HttpException('teigeodata: HTTP ${response.statusCode} for $url', uri: url);
    }
    await response.pipe(part.openWrite());
  } finally {
    client.close();
  }
  final got = await _sha256(part);
  if (got != want) {
    part.deleteSync();
    throw StateError('teigeodata: $url has SHA-256 $got, not the pinned $want; not used');
  }
  // Moved into place whole, so a concurrent build never sees half a file.
  part.renameSync(file.path);
  return file;
}

/// The version in the package's pubspec: the release whose libraries it uses.
String packageVersion(Uri packageRoot) {
  final pubspec = File.fromUri(packageRoot.resolve('pubspec.yaml')).readAsStringSync();
  return RegExp(r'^version:\s*(\S+)', multiLine: true).firstMatch(pubspec)!.group(1)!;
}

Future<String> _sha256(File file) async => (await sha256.bind(file.openRead()).first).toString();
