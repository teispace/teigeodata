// Reads a result blob (format v1, or v0; D-275) in place. Hand-written: the blob format is not
// part of the IDL. The same shape as every language's reader (D-322).
import Foundation

/// A value of a result: a number in the widest type of its kind, or a string.
public enum Value: Sendable, Equatable, CustomStringConvertible {
    case int(Int64)
    case uint(UInt64)
    case double(Double)
    case string(String)

    /// The value as Swift prints it: a string bare, a number in its shortest form.
    public var description: String {
        switch self {
        case .int(let v): String(v)
        case .uint(let v): String(v)
        case .double(let v): String(v)
        case .string(let v): v
        }
    }

    /// A number as a `Double`; `nil` for a string.
    public var double: Double? {
        switch self {
        case .int(let v): Double(v)
        case .uint(let v): Double(v)
        case .double(let v): v
        case .string: nil
        }
    }

    /// An integer as an `Int`; `nil` for a string, a fraction or a value outside `Int`.
    public var int: Int? {
        switch self {
        case .int(let v): Int(exactly: v)
        case .uint(let v): Int(exactly: v)
        case .double(let v): Int(exactly: v)
        case .string: nil
        }
    }

    /// A string; `nil` for a number.
    public var string: String? {
        if case .string(let v) = self { v } else { nil }
    }
}

/// One row of a result: column name to value, `nil` where the row is null.
public struct Record: Sendable, Equatable, CustomStringConvertible {
    /// The values by column name; a null value is absent.
    public let values: [String: Value]

    /// The value of `column`, `nil` where null or where there is no such column.
    public subscript(column: String) -> Value? { values[column] }

    /// A string column's value.
    public func string(_ column: String) -> String? { values[column]?.string }
    /// A number column's value as a `Double`.
    public func double(_ column: String) -> Double? { values[column]?.double }
    /// An integer column's value as an `Int`.
    public func int(_ column: String) -> Int? { values[column]?.int }

    public var description: String { values.description }
}

/// The type of a blob column (format v1).
public enum ColumnType: UInt32, Sendable {
    case i32 = 1, u32, i64, u64, f32, f64, utf8, u8, i16, u16, dict16, dict32

    /// Bytes per row of the column's data: an offset for UTF-8, a code for a dictionary.
    var width: Int {
        switch self {
        case .u8: 1
        case .i16, .u16, .dict16: 2
        case .i32, .u32, .f32, .utf8, .dict32: 4
        case .i64, .u64, .f64: 8
        }
    }

    var isString: Bool { self == .utf8 || self == .dict16 || self == .dict32 }
}

/// A number type a column holds, read little-endian from the blob.
public protocol BlobNumber: Sendable {
    static var columnType: ColumnType { get }
    static func load(_ raw: UnsafeRawBufferPointer, _ at: Int) -> Self
}

extension Int32: BlobNumber {
    public static var columnType: ColumnType { .i32 }
    public static func load(_ raw: UnsafeRawBufferPointer, _ at: Int) -> Self { .init(littleEndian: raw.loadUnaligned(fromByteOffset: at, as: Self.self)) }
}
extension UInt32: BlobNumber {
    public static var columnType: ColumnType { .u32 }
    public static func load(_ raw: UnsafeRawBufferPointer, _ at: Int) -> Self { .init(littleEndian: raw.loadUnaligned(fromByteOffset: at, as: Self.self)) }
}
extension Int64: BlobNumber {
    public static var columnType: ColumnType { .i64 }
    public static func load(_ raw: UnsafeRawBufferPointer, _ at: Int) -> Self { .init(littleEndian: raw.loadUnaligned(fromByteOffset: at, as: Self.self)) }
}
extension UInt64: BlobNumber {
    public static var columnType: ColumnType { .u64 }
    public static func load(_ raw: UnsafeRawBufferPointer, _ at: Int) -> Self { .init(littleEndian: raw.loadUnaligned(fromByteOffset: at, as: Self.self)) }
}
extension UInt8: BlobNumber {
    public static var columnType: ColumnType { .u8 }
    public static func load(_ raw: UnsafeRawBufferPointer, _ at: Int) -> Self { raw[at] }
}
extension Int16: BlobNumber {
    public static var columnType: ColumnType { .i16 }
    public static func load(_ raw: UnsafeRawBufferPointer, _ at: Int) -> Self { .init(littleEndian: raw.loadUnaligned(fromByteOffset: at, as: Self.self)) }
}
extension UInt16: BlobNumber {
    public static var columnType: ColumnType { .u16 }
    public static func load(_ raw: UnsafeRawBufferPointer, _ at: Int) -> Self { .init(littleEndian: raw.loadUnaligned(fromByteOffset: at, as: Self.self)) }
}
extension Float: BlobNumber {
    public static var columnType: ColumnType { .f32 }
    public static func load(_ raw: UnsafeRawBufferPointer, _ at: Int) -> Self { .init(bitPattern: UInt32.load(raw, at)) }
}
extension Double: BlobNumber {
    public static var columnType: ColumnType { .f64 }
    public static func load(_ raw: UnsafeRawBufferPointer, _ at: Int) -> Self { .init(bitPattern: UInt64.load(raw, at)) }
}

/// Reads a result blob: its rows and named columns. Every `Data` result of `Engine` is one;
/// the initializer checks the layout, so a read never leaves the blob.
///
/// ```swift
/// for place in try Reader(engine.search("Kathmandu", limit: 5)) {
///     print(place.string("name")!, place.double("lat")!)
/// }
/// ```
public struct Reader: Sendable, Sequence {
    struct Column: Sendable {
        let name: String
        let type: ColumnType
        let at: Int, length: Int, auxAt: Int, auxLength: Int, nullsAt: Int
    }

    private let data: Data
    /// The number of rows.
    public let rows: Int
    private let layout: [Column]

    /// Checks `data`'s header and column descriptors. Throws `corrupt` for bytes that are not a
    /// well-formed blob.
    public init(_ data: Data) throws(TeiGeodataError) {
        let data = data.startIndex == 0 ? data : Data(data)
        let parsed: (Int, [Column])? = data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in Reader.parse(raw) }
        guard let (rows, columns) = parsed else {
            throw TeiGeodataError(code: .corrupt, message: "not a well-formed result blob")
        }
        self.data = data
        self.rows = rows
        self.layout = columns
    }

    private static func parse(_ raw: UnsafeRawBufferPointer) -> (Int, [Column])? {
        let size = raw.count
        guard size >= 16 else { return nil }
        let magic = Array(raw[0..<4])
        let v0 = magic == Array("TGB0".utf8)
        guard v0 || magic == Array("TGB1".utf8) else { return nil }
        let u32 = { (at: Int) in Int(UInt32.load(raw, at)) }
        let rows = u32(4), count = u32(8)
        // At least one column: with none, nothing would bound the row count.
        guard u32(12) == size, count > 0, count <= (size - 16) / 40 else { return nil }
        let floor = 16 + count * 40
        let inside = { (at: Int, n: Int) in at >= floor && at % 8 == 0 && n >= 0 && at <= size && n <= size - at }
        var columns: [Column] = []
        for i in 0..<count {
            let d = 16 + 40 * i
            let nameBytes = raw[d..<d + 16]
            let end = nameBytes.firstIndex(of: 0) ?? d + 16
            guard end > d, raw[end..<d + 16].allSatisfy({ $0 == 0 }),
                  let type = ColumnType(rawValue: UInt32(u32(d + 16))) else { return nil }
            let name = String(decoding: raw[d..<end], as: UTF8.self)
            guard name.utf8.elementsEqual(raw[d..<end]) else { return nil }
            let c = Column(name: name, type: type, at: u32(d + 20), length: u32(d + 24),
                           auxAt: u32(d + 28), auxLength: u32(d + 32), nullsAt: u32(d + 36))
            let values = type == .utf8 ? rows + 1 : rows
            guard !columns.contains(where: { $0.name == name }),
                  !(v0 && (c.nullsAt != 0 || type.rawValue > ColumnType.utf8.rawValue)),
                  c.length == values * type.width, inside(c.at, c.length),
                  type.isString ? inside(c.auxAt, c.auxLength) : (c.auxAt == 0 && c.auxLength == 0),
                  c.nullsAt == 0 || inside(c.nullsAt, (rows + 7) / 8)
            else { return nil }
            columns.append(c)
        }
        return (rows, columns)
    }

    /// The column names, in order.
    public var columns: [String] { layout.map(\.name) }

    /// Whether there is a column called `name`.
    public func has(_ name: String) -> Bool { layout.contains { $0.name == name } }

    /// The type of the column called `name`.
    public func type(_ name: String) -> ColumnType? { layout.first { $0.name == name }?.type }

    private func described(_ name: String) throws(TeiGeodataError) -> Column {
        guard let c = layout.first(where: { $0.name == name }) else {
            throw TeiGeodataError(code: .invalidArgument, message: "no column \(name)")
        }
        return c
    }

    /// Per row, whether the column called `name` is null there.
    public func nulls(_ name: String) throws(TeiGeodataError) -> [Bool] {
        let c = try described(name)
        guard c.nullsAt != 0 else { return Array(repeating: false, count: rows) }
        return data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) -> [Bool] in
            (0..<rows).map { (r: Int) -> Bool in
                let byte: UInt8 = raw[c.nullsAt + r / 8]
                return (byte >> UInt8(r % 8)) & 1 == 0
            }
        }
    }

    /// The number column called `name`, which must hold `T`; a null row reads as zero (see
    /// `nulls`).
    public func values<T: BlobNumber>(_ name: String, as _: T.Type = T.self) throws(TeiGeodataError) -> [T] {
        let c = try described(name)
        guard c.type == T.columnType else {
            throw TeiGeodataError(code: .invalidArgument, message: "column \(name) does not hold \(T.self)")
        }
        let width = c.type.width
        return data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) -> [T] in (0..<rows).map { T.load(raw, c.at + $0 * width) } }
    }

    /// Any number column called `name` as `Double`s; a null row reads as zero (see `nulls`).
    public func numbers(_ name: String) throws(TeiGeodataError) -> [Double] {
        try numberValues(described(name)).map { $0.double ?? 0 }
    }

    /// The string column called `name` (UTF-8 or a dictionary), `nil` where null.
    public func strings(_ name: String) throws(TeiGeodataError) -> [String?] {
        let c = try described(name)
        guard c.type.isString else {
            throw TeiGeodataError(code: .invalidArgument, message: "column \(name) does not hold strings")
        }
        let nulls = try self.nulls(name)
        let read: [String?]? = data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) -> [String?]? in
            let u32 = { (at: Int) in Int(UInt32.load(raw, at)) }
            let text = { (from: Int, to: Int, limit: Int) -> String? in
                guard from <= to, to <= limit else { return nil }
                return String(decoding: raw[from..<to], as: UTF8.self)
            }
            let auxEnd = c.auxAt + c.auxLength
            if c.type == .utf8 {
                var out: [String?] = []
                out.reserveCapacity(rows)
                for r in 0..<rows {
                    guard let s = text(c.auxAt + u32(c.at + 4 * r), c.auxAt + u32(c.at + 4 * r + 4), auxEnd) else { return nil }
                    out.append(nulls[r] ? nil : s)
                }
                return out
            }
            guard c.auxLength >= 8 else { return nil }
            let count = u32(c.auxAt)
            guard count <= (c.auxLength - 8) / 4 else { return nil }
            let base = c.auxAt + 8 + 4 * count
            var words = [String?](repeating: nil, count: count)
            var out: [String?] = []
            out.reserveCapacity(rows)
            for r in 0..<rows {
                if nulls[r] { out.append(nil); continue }
                let code = c.type == .dict16 ? Int(UInt16.load(raw, c.at + 2 * r)) : u32(c.at + 4 * r)
                guard code < count else { return nil }
                if words[code] == nil {
                    guard let s = text(base + u32(c.auxAt + 4 + 4 * code), base + u32(c.auxAt + 8 + 4 * code), auxEnd) else { return nil }
                    words[code] = s
                }
                out.append(words[code])
            }
            return out
        }
        guard let read else { throw TeiGeodataError(code: .corrupt, message: "column \(name) is malformed") }
        return read
    }

    private func numberValues(_ c: Column) throws(TeiGeodataError) -> [Value] {
        switch c.type {
        case .i32: try values(c.name, as: Int32.self).map { .int(Int64($0)) }
        case .i64: try values(c.name, as: Int64.self).map { .int($0) }
        case .i16: try values(c.name, as: Int16.self).map { .int(Int64($0)) }
        case .u32: try values(c.name, as: UInt32.self).map { .uint(UInt64($0)) }
        case .u64: try values(c.name, as: UInt64.self).map { .uint($0) }
        case .u8: try values(c.name, as: UInt8.self).map { .uint(UInt64($0)) }
        case .u16: try values(c.name, as: UInt16.self).map { .uint(UInt64($0)) }
        case .f32: try values(c.name, as: Float.self).map { .double(Double($0)) }
        case .f64: try values(c.name, as: Double.self).map { .double($0) }
        case .utf8, .dict16, .dict32:
            throw TeiGeodataError(code: .invalidArgument, message: "column \(c.name) does not hold numbers")
        }
    }

    /// The column called `name` with one value per row, `nil` where null.
    public func column(_ name: String) throws(TeiGeodataError) -> [Value?] {
        let c = try described(name)
        if c.type.isString { return try strings(name).map { $0.map(Value.string) } }
        let nulls = try self.nulls(name)
        return try numberValues(c).enumerated().map { nulls[$0.offset] ? nil : $0.element }
    }

    /// Every row as a `Record`.
    public func records() throws(TeiGeodataError) -> [Record] {
        var values = [[String: Value]](repeating: [:], count: rows)
        for c in layout {
            for (r, v) in try column(c.name).enumerated() where v != nil {
                values[r][c.name] = v
            }
        }
        return values.map(Record.init)
    }

    /// The rows as `records()` gives them. The initializer checked the layout, so reading
    /// cannot fail but on a bug; a failure there stops with its message.
    public func makeIterator() -> IndexingIterator<[Record]> {
        do { return try records().makeIterator() } catch { preconditionFailure("teigeodata: \(error)") }
    }
}
