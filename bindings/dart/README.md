# teigeodata for Dart and Flutter

Accurate, offline places: coordinates, elevation, the admin hierarchy, local names, time
zones and postal codes, from signed, compact packs that the library downloads, verifies and
reads on the device.

```sh
dart pub add teigeodata
```

Dart 3.10 or later (Flutter 3.38 or later), on Android, iOS, macOS, Linux and Windows. The
package's build hook supplies the native library: the release's prebuilt library for the
target, checked against the SHA-256 this version pins, or a library you build or name:

```yaml
# pubspec.yaml of your app
hooks:
  user_defines:
    teigeodata:
      # library: path/to/libteigeodata.so   # a library already built
      # source: path/to/teigeodata          # build from a checkout with cargo
      # prebuilt_base: https://mirror.example/teigeodata   # a mirror of the releases
```

```dart
import 'package:teigeodata/teigeodata.dart';

final tg = TeiGeodata();
final engine = Engine(tg);
engine.openFile('np.tgd');
final blob = engine.search('Kathmandu', 5); // a TGB1 result blob
```

Queries return their rows as one columnar blob (`TGB1`), read in place:
[the format](https://github.com/teispace/teigeodata/blob/main/docs/04-architecture/bindings.md).
A failure throws a `TeiGeodataException` subclass with a stable code
(`MissingComponentException`, …). Blocking store calls have `…Async` forms, and a job streams
its progress.

Licence: Apache-2.0 for the code; the data packs carry their own licences and attribution.
Design: [D-289](https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/log.md).
