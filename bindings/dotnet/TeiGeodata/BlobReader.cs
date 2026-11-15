#nullable enable

using System;
using System.Buffers.Binary;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

namespace Teispace.TeiGeodata;

/// <summary>The type of a blob column (format v1, D-275).</summary>
public enum ColumnType : uint
{
    /// <summary>32-bit signed integers.</summary>
    I32 = 1,
    /// <summary>32-bit unsigned integers.</summary>
    U32 = 2,
    /// <summary>64-bit signed integers.</summary>
    I64 = 3,
    /// <summary>64-bit unsigned integers.</summary>
    U64 = 4,
    /// <summary>32-bit floats.</summary>
    F32 = 5,
    /// <summary>64-bit floats.</summary>
    F64 = 6,
    /// <summary>UTF-8 strings.</summary>
    Utf8 = 7,
    /// <summary>Bytes.</summary>
    U8 = 8,
    /// <summary>16-bit signed integers.</summary>
    I16 = 9,
    /// <summary>16-bit unsigned integers.</summary>
    U16 = 10,
    /// <summary>Strings from a dictionary, by 16-bit codes.</summary>
    Dict16 = 11,
    /// <summary>Strings from a dictionary, by 32-bit codes.</summary>
    Dict32 = 12,
}

/// <summary>
/// Reads a result blob (format v1, or v0) in place: its rows and named columns. Every blob a
/// call returns is one; the constructor checks the layout, and each column is checked again
/// when it is read. Number columns are views of the blob, strings are copied when read (a
/// dictionary's strings once each).
/// </summary>
/// <example>
/// <code>
/// var places = new BlobReader(engine.Search("Kathmandu", 5));
/// string[] names = places.Strings("name");
/// ReadOnlySpan&lt;double&gt; lats = places.Values&lt;double&gt;("lat");
/// foreach (var place in places) // each row: column name to value, null where null
/// {
///     Console.WriteLine($"{place["name"]} {place["lat"]}");
/// }
/// </code>
/// </example>
public sealed class BlobReader : IEnumerable<IReadOnlyDictionary<string, object?>>
{
    private const int Header = 16;
    private const int Descriptor = 40;
    private const int NameMax = 16;

    private readonly byte[] data;
    private readonly ColumnInfo[] columns;

    /// <summary>Checks <paramref name="data"/>'s header and column descriptors.</summary>
    /// <exception cref="InvalidDataException">The bytes are not a well-formed blob.</exception>
    public BlobReader(byte[] data)
    {
        ArgumentNullException.ThrowIfNull(data);
        this.data = data;
        if (data.Length < Header)
        {
            throw Malformed();
        }
        ReadOnlySpan<byte> magic = data.AsSpan(0, 4);
        bool v0 = magic.SequenceEqual("TGB0"u8);
        if (!v0 && !magic.SequenceEqual("TGB1"u8))
        {
            throw Malformed();
        }
        uint rows = U32(data, 4);
        uint count = U32(data, 8);
        if (U32(data, 12) != (uint)data.Length || count > (uint)((data.Length - Header) / Descriptor) || rows > int.MaxValue - 1)
        {
            throw Malformed();
        }
        Rows = (int)rows;
        long floor = Header + (long)count * Descriptor;
        columns = new ColumnInfo[count];
        for (int i = 0; i < count; i++)
        {
            ReadOnlySpan<byte> d = data.AsSpan(Header + i * Descriptor, Descriptor);
            int end = d[..NameMax].IndexOf((byte)0);
            if (end < 0)
            {
                end = NameMax;
            }
            if (d[end..NameMax].IndexOfAnyExcept((byte)0) >= 0)
            {
                throw Malformed();
            }
            var c = new ColumnInfo(
                Encoding.UTF8.GetString(d[..end]),
                (ColumnType)U32(d, NameMax),
                U32(d, NameMax + 4),
                U32(d, NameMax + 8),
                U32(d, NameMax + 12),
                U32(d, NameMax + 16),
                U32(d, NameMax + 20));
            long width = Width(c.Type);
            long values = c.Type == ColumnType.Utf8 ? (long)Rows + 1 : Rows;
            bool hasAux = c.Type is ColumnType.Utf8 or ColumnType.Dict16 or ColumnType.Dict32;
            bool bad = end == 0 || width == 0 || Index(c.Name, i) >= 0
                || (v0 && (c.NullsAt != 0 || c.Type > ColumnType.Utf8))
                || c.Length != values * width || !Region(c.At, c.Length, floor)
                || (hasAux && !Region(c.AuxAt, c.AuxLength, floor))
                || (!hasAux && (c.AuxAt != 0 || c.AuxLength != 0))
                || (c.NullsAt != 0 && !Region(c.NullsAt, (Rows + 7L) / 8, floor));
            if (bad)
            {
                throw Malformed();
            }
            columns[i] = c;
        }
    }

    /// <summary>The number of rows.</summary>
    public int Rows { get; }

    /// <summary>The column names, in order.</summary>
    public IReadOnlyList<string> Columns => Array.ConvertAll(columns, c => c.Name);

    /// <summary>Whether there is a column called <paramref name="name"/>.</summary>
    public bool Has(string name) => Index(name, columns.Length) >= 0;

    /// <summary>The type of the column called <paramref name="name"/>, if there is one.</summary>
    public bool TryGetType(string name, out ColumnType type)
    {
        int i = Index(name, columns.Length);
        type = i >= 0 ? columns[i].Type : default;
        return i >= 0;
    }

    /// <summary>Per row, whether the column is null there; null when no row is.</summary>
    /// <exception cref="KeyNotFoundException">The blob has no such column.</exception>
    public bool[]? Nulls(string name)
    {
        ColumnInfo c = Find(name);
        if (c.NullsAt == 0)
        {
            return null;
        }
        ReadOnlySpan<byte> bits = data.AsSpan((int)c.NullsAt, (Rows + 7) / 8);
        var nulls = new bool[Rows];
        for (int i = 0; i < nulls.Length; i++)
        {
            nulls[i] = (bits[i >> 3] >> (i & 7) & 1) == 0;
        }
        return nulls;
    }

    /// <summary>
    /// The number column called <paramref name="name"/>, which must hold <typeparamref name="T"/>
    /// (a null row reads as zero: see <see cref="Nulls"/>). A view of the blob, without a copy.
    /// </summary>
    /// <exception cref="KeyNotFoundException">The blob has no such column.</exception>
    /// <exception cref="InvalidCastException">The column holds another type.</exception>
    public ReadOnlySpan<T> Values<T>(string name)
        where T : unmanaged
    {
        ColumnInfo c = Find(name);
        if (c.Type != TypeOf<T>())
        {
            throw new InvalidCastException($"teigeodata: column {name} holds {c.Type}, not {typeof(T).Name}");
        }
        ReadOnlySpan<T> values = MemoryMarshal.Cast<byte, T>(data.AsSpan((int)c.At, (int)c.Length));
        if (BitConverter.IsLittleEndian || c.Type == ColumnType.U8)
        {
            return values;
        }
        var swapped = new byte[c.Length];
        data.AsSpan((int)c.At, (int)c.Length).CopyTo(swapped);
        int size = (int)Width(c.Type);
        for (int i = 0; i < swapped.Length; i += size)
        {
            swapped.AsSpan(i, size).Reverse();
        }
        return MemoryMarshal.Cast<byte, T>(swapped);
    }

    /// <summary>The string column called <paramref name="name"/> (UTF-8 or a dictionary); a
    /// null row reads as "" (see <see cref="Nulls"/>).</summary>
    /// <exception cref="KeyNotFoundException">The blob has no such column.</exception>
    /// <exception cref="InvalidCastException">The column does not hold strings.</exception>
    /// <exception cref="InvalidDataException">The column's offsets or codes are out of range.</exception>
    public string[] Strings(string name)
    {
        ColumnInfo c = Find(name);
        ReadOnlySpan<byte> values = data.AsSpan((int)c.At, (int)c.Length);
        ReadOnlySpan<byte> aux = data.AsSpan((int)c.AuxAt, (int)c.AuxLength);
        var output = new string[Rows];
        switch (c.Type)
        {
            case ColumnType.Utf8:
                for (int i = 0; i < output.Length; i++)
                {
                    output[i] = Slice(aux, values, i);
                }
                return output;
            case ColumnType.Dict16:
            case ColumnType.Dict32:
                if (aux.Length < 8)
                {
                    throw Malformed();
                }
                uint count = U32(aux, 0);
                if (count > (uint)((aux.Length - 8) / 4))
                {
                    throw Malformed();
                }
                ReadOnlySpan<byte> offsets = aux[4..(8 + 4 * (int)count)];
                ReadOnlySpan<byte> strings = aux[(8 + 4 * (int)count)..];
                var words = new string?[count];
                for (int i = 0; i < output.Length; i++)
                {
                    uint code = c.Type == ColumnType.Dict16
                        ? BinaryPrimitives.ReadUInt16LittleEndian(values[(2 * i)..])
                        : U32(values, 4 * i);
                    if (code >= count)
                    {
                        if (code == 0)
                        {
                            output[i] = "";
                            continue; // a null row of an empty dictionary
                        }
                        throw Malformed();
                    }
                    output[i] = words[code] ??= Slice(strings, offsets, (int)code);
                }
                return output;
            default:
                throw new InvalidCastException($"teigeodata: column {name} holds {c.Type}, not strings");
        }
    }

    /// <summary>
    /// Any number column as doubles (a null row reads as zero: see <see cref="Nulls"/>); a
    /// 64-bit integer is exact to 2^53.
    /// </summary>
    /// <exception cref="KeyNotFoundException">The blob has no such column.</exception>
    /// <exception cref="InvalidCastException">The column holds strings.</exception>
    public double[] Numbers(string name)
    {
        object[] values = NumberValues(Find(name));
        var output = new double[values.Length];
        for (int i = 0; i < values.Length; i++)
        {
            output[i] = Convert.ToDouble(values[i], CultureInfo.InvariantCulture);
        }
        return output;
    }

    /// <summary>
    /// The column called <paramref name="name"/> with one value per row: a
    /// <see cref="string"/>, or a number in the column's own type (<see cref="int"/>,
    /// <see cref="byte"/>, <see cref="double"/>, ...), and null where the row is null.
    /// </summary>
    /// <exception cref="KeyNotFoundException">The blob has no such column.</exception>
    public object?[] Column(string name)
    {
        ColumnInfo c = Find(name);
        var output = new object?[Rows];
        if (c.Type is ColumnType.Utf8 or ColumnType.Dict16 or ColumnType.Dict32)
        {
            string[] strings = Strings(name);
            Array.Copy(strings, output, Rows);
        }
        else
        {
            object[] numbers = NumberValues(c);
            Array.Copy(numbers, output, Rows);
        }
        bool[]? nulls = Nulls(name);
        if (nulls is not null)
        {
            for (int i = 0; i < Rows; i++)
            {
                if (nulls[i])
                {
                    output[i] = null;
                }
            }
        }
        return output;
    }

    /// <summary>Every row as a dictionary from column name to value, as <see cref="Column"/>
    /// reads it.</summary>
    public IReadOnlyList<IReadOnlyDictionary<string, object?>> Records()
    {
        var values = new object?[columns.Length][];
        for (int k = 0; k < columns.Length; k++)
        {
            values[k] = Column(columns[k].Name);
        }
        var output = new IReadOnlyDictionary<string, object?>[Rows];
        for (int i = 0; i < Rows; i++)
        {
            var row = new Dictionary<string, object?>(columns.Length);
            for (int k = 0; k < columns.Length; k++)
            {
                row[columns[k].Name] = values[k][i];
            }
            output[i] = row;
        }
        return output;
    }

    /// <summary>The rows as <see cref="Records"/> gives them.</summary>
    public IEnumerator<IReadOnlyDictionary<string, object?>> GetEnumerator() => Records().GetEnumerator();

    IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();

    private object[] NumberValues(ColumnInfo c) => c.Type switch
    {
        ColumnType.I32 => Box(Values<int>(c.Name)),
        ColumnType.U32 => Box(Values<uint>(c.Name)),
        ColumnType.I64 => Box(Values<long>(c.Name)),
        ColumnType.U64 => Box(Values<ulong>(c.Name)),
        ColumnType.F32 => Box(Values<float>(c.Name)),
        ColumnType.F64 => Box(Values<double>(c.Name)),
        ColumnType.U8 => Box(Values<byte>(c.Name)),
        ColumnType.I16 => Box(Values<short>(c.Name)),
        ColumnType.U16 => Box(Values<ushort>(c.Name)),
        _ => throw new InvalidCastException($"teigeodata: column {c.Name} holds {c.Type}, not numbers"),
    };

    private static object[] Box<T>(ReadOnlySpan<T> values)
        where T : unmanaged
    {
        var output = new object[values.Length];
        for (int i = 0; i < values.Length; i++)
        {
            output[i] = values[i];
        }
        return output;
    }

    private static string Slice(ReadOnlySpan<byte> bytes, ReadOnlySpan<byte> offsets, int i)
    {
        uint at = U32(offsets, 4 * i);
        uint end = U32(offsets, 4 * i + 4);
        if (at > end || end > (uint)bytes.Length)
        {
            throw Malformed();
        }
        return Encoding.UTF8.GetString(bytes[(int)at..(int)end]);
    }

    private bool Region(uint at, long length, long floor) =>
        at >= floor && at % 8 == 0 && length >= 0 && at <= data.Length && length <= data.Length - at;

    private int Index(string name, int before)
    {
        for (int i = 0; i < before; i++)
        {
            if (columns[i].Name == name)
            {
                return i;
            }
        }
        return -1;
    }

    private ColumnInfo Find(string name)
    {
        int i = Index(name, columns.Length);
        return i >= 0 ? columns[i] : throw new KeyNotFoundException($"teigeodata: the blob has no column {name}");
    }

    private static long Width(ColumnType type) => type switch
    {
        ColumnType.U8 => 1,
        ColumnType.I16 or ColumnType.U16 or ColumnType.Dict16 => 2,
        ColumnType.I32 or ColumnType.U32 or ColumnType.F32 or ColumnType.Utf8 or ColumnType.Dict32 => 4,
        ColumnType.I64 or ColumnType.U64 or ColumnType.F64 => 8,
        _ => 0,
    };

    private static ColumnType TypeOf<T>()
        where T : unmanaged
    {
        if (typeof(T) == typeof(int)) return ColumnType.I32;
        if (typeof(T) == typeof(uint)) return ColumnType.U32;
        if (typeof(T) == typeof(long)) return ColumnType.I64;
        if (typeof(T) == typeof(ulong)) return ColumnType.U64;
        if (typeof(T) == typeof(float)) return ColumnType.F32;
        if (typeof(T) == typeof(double)) return ColumnType.F64;
        if (typeof(T) == typeof(byte)) return ColumnType.U8;
        if (typeof(T) == typeof(short)) return ColumnType.I16;
        if (typeof(T) == typeof(ushort)) return ColumnType.U16;
        return 0;
    }

    private static uint U32(ReadOnlySpan<byte> bytes, int at) => BinaryPrimitives.ReadUInt32LittleEndian(bytes[at..]);

    private static InvalidDataException Malformed() => new("teigeodata: malformed blob");

    private readonly record struct ColumnInfo(string Name, ColumnType Type, uint At, uint Length, uint AuxAt, uint AuxLength, uint NullsAt);
}
