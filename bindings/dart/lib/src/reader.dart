// Reads a result blob (format v1, or v0; D-275) in place. Hand-written: the blob format is not
// part of the IDL. Pure Dart, so the same code reads results natively and on the web. The same
// shape as every language's reader (D-322).
library;

import 'dart:collection';
import 'dart:convert';
import 'dart:typed_data';

import 'types.g.dart';

const _utf8Type = 7, _dict16 = 11, _dict32 = 12;

/// Bytes per row of a column's data, by type: an offset for UTF-8, a code for a dictionary.
int _width(int type) => switch (type) {
  8 => 1,
  9 || 10 || 11 => 2,
  1 || 2 || 5 || 7 || 12 => 4,
  3 || 4 || 6 => 8,
  _ => 0,
};

bool _isString(int type) => type == _utf8Type || type == _dict16 || type == _dict32;

final class _Column {
  const _Column(
    this.name,
    this.type,
    this.at,
    this.length,
    this.auxAt,
    this.auxLength,
    this.nullsAt,
  );
  final String name;
  final int type, at, length, auxAt, auxLength, nullsAt;
}

/// Reads a result blob: its rows and named columns. Every `Uint8List` result of [Engine] is
/// one; the constructor checks the layout, so a read never leaves the blob.
///
/// Iterating a reader gives each row as a map from column name to value: a `String`, an
/// `int` or a `double`, and `null` where the row is null.
///
/// ```dart
/// for (final place in Reader(engine.search('Kathmandu', 5))) {
///   print('${place['name']} ${place['lat']}');
/// }
/// ```
final class Reader with IterableMixin<Map<String, Object?>> {
  /// Checks [blob]'s header and column descriptors; throws [CorruptException] for bytes that
  /// are not a well-formed blob.
  factory Reader(Uint8List blob) {
    final v = ByteData.sublistView(blob);
    final size = blob.length;
    Never bad() => throw const CorruptException('not a well-formed result blob');
    if (size < 16) bad();
    final magic = ascii.decode(blob.sublist(0, 4), allowInvalid: true);
    final v0 = magic == 'TGB0';
    if (!v0 && magic != 'TGB1') bad();
    final rows = v.getUint32(4, Endian.little), count = v.getUint32(8, Endian.little);
    // At least one column: with none, nothing would bound the row count.
    if (v.getUint32(12, Endian.little) != size || count == 0 || count > (size - 16) ~/ 40) bad();
    final floor = 16 + count * 40;
    bool inside(int at, int n) => at >= floor && at % 8 == 0 && at <= size && n <= size - at;
    final columns = <_Column>[];
    for (var i = 0; i < count; i++) {
      final d = 16 + 40 * i;
      var end = d;
      while (end < d + 16 && blob[end] != 0) {
        end++;
      }
      if (end == d || blob.sublist(end, d + 16).any((b) => b != 0)) bad();
      final String name;
      try {
        name = utf8.decode(blob.sublist(d, end));
      } on FormatException {
        bad();
      }
      int u32(int at) => v.getUint32(at, Endian.little);
      final c = _Column(
        name,
        u32(d + 16),
        u32(d + 20),
        u32(d + 24),
        u32(d + 28),
        u32(d + 32),
        u32(d + 36),
      );
      final width = _width(c.type);
      final values = c.type == _utf8Type ? rows + 1 : rows;
      final ok =
          width != 0 &&
          !columns.any((o) => o.name == name) &&
          !(v0 && (c.nullsAt != 0 || c.type > _utf8Type)) &&
          c.length == values * width &&
          inside(c.at, c.length) &&
          (_isString(c.type) ? inside(c.auxAt, c.auxLength) : c.auxAt == 0 && c.auxLength == 0) &&
          (c.nullsAt == 0 || inside(c.nullsAt, (rows + 7) ~/ 8));
      if (!ok) bad();
      columns.add(c);
    }
    return Reader._(blob, v, rows, columns);
  }

  Reader._(this._blob, this._view, this.rows, this._columns);

  final Uint8List _blob;
  final ByteData _view;
  final List<_Column> _columns;

  /// The number of rows.
  final int rows;

  /// The column names, in order.
  List<String> get columns => [for (final c in _columns) c.name];

  /// Whether there is a column called [name].
  bool has(String name) => _columns.any((c) => c.name == name);

  _Column _column(String name) => _columns.firstWhere(
    (c) => c.name == name,
    orElse: () => throw InvalidArgumentException('no column $name'),
  );

  /// Per row, whether the column called [name] is null there.
  List<bool> nulls(String name) {
    final c = _column(name);
    if (c.nullsAt == 0) return List.filled(rows, false);
    return [for (var r = 0; r < rows; r++) (_blob[c.nullsAt + (r >> 3)] >> (r & 7)) & 1 == 0];
  }

  /// The number column called [name]; a null row reads as zero (see [nulls]). A 64-bit
  /// integer is exact to 2^53 on the web.
  List<num> numbers(String name) {
    final c = _column(name);
    final at = c.at;
    final v = _view;
    const le = Endian.little;
    return switch (c.type) {
      1 => [for (var r = 0; r < rows; r++) v.getInt32(at + 4 * r, le)],
      2 => [for (var r = 0; r < rows; r++) v.getUint32(at + 4 * r, le)],
      // Two halves: dart2js has no 64-bit ByteData reads.
      3 => [
        for (var r = 0; r < rows; r++)
          v.getInt32(at + 8 * r + 4, le) * 0x100000000 + v.getUint32(at + 8 * r, le),
      ],
      4 => [
        for (var r = 0; r < rows; r++)
          v.getUint32(at + 8 * r + 4, le) * 0x100000000 + v.getUint32(at + 8 * r, le),
      ],
      5 => [for (var r = 0; r < rows; r++) v.getFloat32(at + 4 * r, le)],
      6 => [for (var r = 0; r < rows; r++) v.getFloat64(at + 8 * r, le)],
      8 => [for (var r = 0; r < rows; r++) v.getUint8(at + r)],
      9 => [for (var r = 0; r < rows; r++) v.getInt16(at + 2 * r, le)],
      10 => [for (var r = 0; r < rows; r++) v.getUint16(at + 2 * r, le)],
      _ => throw InvalidArgumentException('column $name does not hold numbers'),
    };
  }

  /// The string column called [name] (UTF-8 or a dictionary), `null` where null.
  List<String?> strings(String name) {
    final c = _column(name);
    if (!_isString(c.type)) throw InvalidArgumentException('column $name does not hold strings');
    final nulls = this.nulls(name);
    final v = _view;
    int u32(int at) => v.getUint32(at, Endian.little);
    final auxEnd = c.auxAt + c.auxLength;
    String text(int from, int to) {
      if (from > to || to > auxEnd) throw CorruptException('column $name is malformed');
      return utf8.decode(Uint8List.sublistView(_blob, from, to), allowMalformed: true);
    }

    if (c.type == _utf8Type) {
      return [
        for (var r = 0; r < rows; r++)
          nulls[r] ? null : text(c.auxAt + u32(c.at + 4 * r), c.auxAt + u32(c.at + 4 * r + 4)),
      ];
    }
    if (c.auxLength < 8) throw CorruptException('column $name is malformed');
    final count = u32(c.auxAt);
    if (count > (c.auxLength - 8) ~/ 4) throw CorruptException('column $name is malformed');
    final base = c.auxAt + 8 + 4 * count;
    final words = List<String?>.filled(count, null);
    return [
      for (var r = 0; r < rows; r++)
        if (nulls[r])
          null
        else
          () {
            final code = c.type == _dict16
                ? v.getUint16(c.at + 2 * r, Endian.little)
                : u32(c.at + 4 * r);
            if (code >= count) throw CorruptException('column $name is malformed');
            return words[code] ??= text(
              base + u32(c.auxAt + 4 + 4 * code),
              base + u32(c.auxAt + 8 + 4 * code),
            );
          }(),
    ];
  }

  /// The column called [name] with one value per row: a `String`, an `int` or a `double`,
  /// `null` where null.
  List<Object?> column(String name) {
    final c = _column(name);
    if (_isString(c.type)) return strings(name);
    final nulls = this.nulls(name);
    final values = numbers(name);
    return [for (var r = 0; r < rows; r++) nulls[r] ? null : values[r]];
  }

  /// Every row as a map from column name to value, as [column] reads it.
  List<Map<String, Object?>> records() {
    final columns = {for (final c in _columns) c.name: column(c.name)};
    return [
      for (var r = 0; r < rows; r++)
        {for (final MapEntry(:key, :value) in columns.entries) key: value[r]},
    ];
  }

  @override
  Iterator<Map<String, Object?>> get iterator => records().iterator;

  @override
  int get length => rows;
}
