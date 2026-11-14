/// teigeodata: accurate, offline places. The API is generated from the Rust crate
/// `teigeodata-api`: natively over its C ABI (`src/bindings.g.dart`), on the web over its
/// WebAssembly module (`src/bindings_web.g.dart`), with the same classes and types.
library;

export 'src/bindings.g.dart' if (dart.library.js_interop) 'src/bindings_web.g.dart';
export 'src/types.g.dart' hide exceptionNamed, exceptionOf;
