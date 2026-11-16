// The target a hook builds for: its Rust triple, the library's file name, and what cargo needs
// to build for it.
import 'dart:io';

import 'package:code_assets/code_assets.dart';

/// One build target.
final class Target {
  const Target._(this.triple, this.os);

  /// The target of a hook's [CodeConfig].
  factory Target.of(CodeConfig code) {
    final os = code.targetOS;
    final triple = switch ((os, code.targetArchitecture)) {
      (OS.android, Architecture.arm64) => 'aarch64-linux-android',
      (OS.android, Architecture.arm) => 'armv7-linux-androideabi',
      (OS.android, Architecture.x64) => 'x86_64-linux-android',
      (OS.iOS, Architecture.arm64) when code.iOS.targetSdk == IOSSdk.iPhoneSimulator =>
        'aarch64-apple-ios-sim',
      (OS.iOS, Architecture.arm64) => 'aarch64-apple-ios',
      (OS.iOS, Architecture.x64) => 'x86_64-apple-ios',
      (OS.macOS, Architecture.arm64) => 'aarch64-apple-darwin',
      (OS.macOS, Architecture.x64) => 'x86_64-apple-darwin',
      (OS.linux, Architecture.arm64) => 'aarch64-unknown-linux-gnu',
      (OS.linux, Architecture.x64) => 'x86_64-unknown-linux-gnu',
      (OS.windows, Architecture.arm64) => 'aarch64-pc-windows-msvc',
      (OS.windows, Architecture.x64) => 'x86_64-pc-windows-msvc',
      (final o, final a) => throw UnsupportedError('teigeodata: no native library for $o on $a'),
    };
    return Target._(triple, os);
  }

  /// The Rust target triple.
  final String triple;
  final OS os;

  /// The library file's name without its extension, as cargo names it.
  String get stem => os == OS.windows ? 'teigeodata' : 'libteigeodata';

  /// The library file's name.
  String get fileName => switch (os) {
    OS.windows => 'teigeodata.dll',
    OS.macOS || OS.iOS => 'libteigeodata.dylib',
    _ => 'libteigeodata.so',
  };

  /// The environment cargo runs in: the hook's own (already reduced to what builds need), the
  /// Apple deployment target, and on Android the NDK's compiler for the app's lowest API level.
  Map<String, String> cargoEnvironment(CodeConfig code) {
    final env = <String, String>{};
    if (Platform.isMacOS) {
      // Xcode puts its own tools first on PATH when it runs the hook; a build script that runs
      // `cc` for the host would then get the target's SDK.
      final path = Platform.environment['PATH'] ?? '';
      env['PATH'] = path.split(':').where((p) => !p.contains('/Contents/Developer/')).join(':');
    }
    final key = triple.replaceAll('-', '_');
    switch (os) {
      case OS.macOS:
        env['MACOSX_DEPLOYMENT_TARGET'] = _apple(code.macOS.deploymentVersion);
      case OS.iOS:
        env['IPHONEOS_DEPLOYMENT_TARGET'] = _apple(code.iOS.deploymentVersion);
      case OS.android:
        final c = code.cCompiler;
        if (c == null) throw StateError('teigeodata: the build gave no Android NDK compiler');
        // The NDK's per-API clang wrapper: linked against the app's lowest API level, so the
        // library never needs a newer libc symbol than the app's oldest device has.
        final bin = File.fromUri(c.compiler).parent.path;
        final api = code.android.targetNdkApi;
        final clangTriple = triple == 'armv7-linux-androideabi'
            ? 'armv7a-linux-androideabi'
            : triple;
        final clang =
            '$bin${Platform.pathSeparator}$clangTriple$api-clang${Platform.isWindows ? '.cmd' : ''}';
        if (!File(clang).existsSync()) {
          throw StateError('teigeodata: no $clang; the NDK is too old or lacks API $api');
        }
        env['CC_$key'] = clang;
        env['AR_$key'] = File.fromUri(c.archiver).path;
        env['CARGO_TARGET_${key.toUpperCase()}_LINKER'] = clang;
      case _:
    }
    return env;
  }
}

String _apple(AppleVersion v) => '${v.major}.${v.minor}';
