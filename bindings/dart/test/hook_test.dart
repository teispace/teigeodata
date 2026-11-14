// The build hook's own logic, without a build.
import 'package:test/test.dart';

import '../hook/build.dart';

void main() {
  test('cargo dependency files: every path, escaped spaces kept', () {
    const text =
        '/r/target/release/libteigeodata.dylib: /r/crates/a/src/lib.rs '
        '/r/my\\ dir/b.rs \\\n /r/crates/c/build.rs\n';
    expect(cargoDependencies(text).map((u) => u.toFilePath()).toList(), [
      '/r/crates/a/src/lib.rs',
      '/r/my dir/b.rs',
      '/r/crates/c/build.rs',
    ]);
  });

  test('a dependency file without a target names nothing', () {
    expect(cargoDependencies(''), isEmpty);
  });
}
