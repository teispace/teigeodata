// Reads a result blob (format v1, or v0; D-275) in place. Hand-written: the blob format is not
// part of the IDL. The same shape as every language's reader (D-322).
package org.teispace.teigeodata;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.CharacterCodingException;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Reads a result blob: its rows and named columns. Every {@code byte[]} result of
 * {@link TeiGeodata.Engine} is one; the constructor checks the layout, so a read never leaves
 * the blob.
 *
 * <p>A value is a {@link String}, an {@link Integer} (columns of {@code i32}, {@code u8},
 * {@code i16} and {@code u16}), a {@link Long} ({@code u32}, {@code i64}, and {@code u64} read
 * as signed), a {@link Float} or a {@link Double}, and {@code null} where the row is null.
 * Iterating a reader gives each row as a map from column name to value, in column order.
 *
 * <pre>{@code
 * for (Map<String, Object> place : new Reader(engine.search("Kathmandu", 5))) {
 *     System.out.println(place.get("name") + " " + place.get("lat"));
 * }
 * }</pre>
 */
public final class Reader implements Iterable<Map<String, Object>> {
    private static final int UTF8 = 7, DICT16 = 11, DICT32 = 12;

    private static final class Column {
        final String name;
        final int type, at, length, auxAt, auxLength, nullsAt;

        Column(String name, int type, int at, int length, int auxAt, int auxLength, int nullsAt) {
            this.name = name;
            this.type = type;
            this.at = at;
            this.length = length;
            this.auxAt = auxAt;
            this.auxLength = auxLength;
            this.nullsAt = nullsAt;
        }
    }

    private final byte[] blob;
    private final ByteBuffer view;
    private final int rows;
    private final List<Column> layout;

    /**
     * Checks {@code blob}'s header and column descriptors.
     *
     * @throws TeiGeodataException.Corrupt for bytes that are not a well-formed blob
     */
    public Reader(byte[] blob) {
        this.blob = blob;
        this.view = ByteBuffer.wrap(blob).order(ByteOrder.LITTLE_ENDIAN);
        int size = blob.length;
        if (size < 16) throw bad();
        String magic = new String(blob, 0, 4, StandardCharsets.ISO_8859_1);
        boolean v0 = magic.equals("TGB0");
        if (!v0 && !magic.equals("TGB1")) throw bad();
        long rows = u32(4), count = u32(8);
        if (u32(12) != size || count > (size - 16) / 40 || rows > Integer.MAX_VALUE) throw bad();
        this.rows = (int) rows;
        long floor = 16 + count * 40;
        List<Column> columns = new ArrayList<>();
        for (int i = 0; i < count; i++) {
            int d = 16 + 40 * i;
            int end = d;
            while (end < d + 16 && blob[end] != 0) end++;
            if (end == d) throw bad();
            for (int k = end; k < d + 16; k++) if (blob[k] != 0) throw bad();
            String name;
            try {
                name = StandardCharsets.UTF_8.newDecoder()
                        .onMalformedInput(CodingErrorAction.REPORT)
                        .decode(ByteBuffer.wrap(blob, d, end - d))
                        .toString();
            } catch (CharacterCodingException e) {
                throw bad();
            }
            long type = u32(d + 16), at = u32(d + 20), length = u32(d + 24);
            long auxAt = u32(d + 28), auxLength = u32(d + 32), nullsAt = u32(d + 36);
            int width = width((int) Math.min(type, 255));
            long values = type == UTF8 ? rows + 1 : rows;
            boolean text = isString((int) Math.min(type, 255));
            boolean ok = width != 0
                    && columns.stream().noneMatch(c -> c.name.equals(name))
                    && !(v0 && (nullsAt != 0 || type > UTF8))
                    && length == values * width
                    && inside(at, length, floor, size)
                    && (text ? inside(auxAt, auxLength, floor, size) : auxAt == 0 && auxLength == 0)
                    && (nullsAt == 0 || inside(nullsAt, (rows + 7) / 8, floor, size));
            if (!ok) throw bad();
            columns.add(new Column(name, (int) type, (int) at, (int) length, (int) auxAt, (int) auxLength, (int) nullsAt));
        }
        this.layout = Collections.unmodifiableList(columns);
    }

    private static TeiGeodataException bad() {
        return new TeiGeodataException.Corrupt("not a well-formed result blob");
    }

    private static boolean inside(long at, long n, long floor, long size) {
        return at >= floor && at % 8 == 0 && at <= size && n <= size - at;
    }

    /** Bytes per row of a column's data: an offset for UTF-8, a code for a dictionary. */
    private static int width(int type) {
        switch (type) {
            case 8:
                return 1;
            case 9: case 10: case 11:
                return 2;
            case 1: case 2: case 5: case 7: case 12:
                return 4;
            case 3: case 4: case 6:
                return 8;
            default:
                return 0;
        }
    }

    private static boolean isString(int type) {
        return type == UTF8 || type == DICT16 || type == DICT32;
    }

    private long u32(int at) {
        return Integer.toUnsignedLong(view.getInt(at));
    }

    /** The number of rows. */
    public int rows() {
        return rows;
    }

    /** The column names, in order. */
    public List<String> columns() {
        List<String> out = new ArrayList<>(layout.size());
        for (Column c : layout) out.add(c.name);
        return out;
    }

    /** Whether there is a column called {@code name}. */
    public boolean has(String name) {
        for (Column c : layout) if (c.name.equals(name)) return true;
        return false;
    }

    private Column column0(String name) {
        for (Column c : layout) if (c.name.equals(name)) return c;
        throw new TeiGeodataException.InvalidArgument("no column " + name);
    }

    /** Per row, whether the column called {@code name} is null there. */
    public boolean[] nulls(String name) {
        Column c = column0(name);
        boolean[] out = new boolean[rows];
        if (c.nullsAt == 0) return out;
        for (int r = 0; r < rows; r++) out[r] = ((blob[c.nullsAt + (r >> 3)] >> (r & 7)) & 1) == 0;
        return out;
    }

    /**
     * The number column called {@code name} as doubles; a null row reads as zero (see
     * {@link #nulls}). A 64-bit integer is exact to 2^53.
     */
    public double[] numbers(String name) {
        Column c = column0(name);
        double[] out = new double[rows];
        for (int r = 0; r < rows; r++) out[r] = number(c, r).doubleValue();
        return out;
    }

    private Number number(Column c, int r) {
        switch (c.type) {
            case 1: return view.getInt(c.at + 4 * r);
            case 2: return Integer.toUnsignedLong(view.getInt(c.at + 4 * r));
            case 3: case 4: return view.getLong(c.at + 8 * r);
            case 5: return view.getFloat(c.at + 4 * r);
            case 6: return view.getDouble(c.at + 8 * r);
            case 8: return Byte.toUnsignedInt(blob[c.at + r]);
            case 9: return (int) view.getShort(c.at + 2 * r);
            case 10: return Short.toUnsignedInt(view.getShort(c.at + 2 * r));
            default: throw new TeiGeodataException.InvalidArgument("column " + c.name + " does not hold numbers");
        }
    }

    /** The string column called {@code name} (UTF-8 or a dictionary), {@code null} where null. */
    public String[] strings(String name) {
        Column c = column0(name);
        if (!isString(c.type)) throw new TeiGeodataException.InvalidArgument("column " + name + " does not hold strings");
        boolean[] nulls = nulls(name);
        String[] out = new String[rows];
        long auxEnd = (long) c.auxAt + c.auxLength;
        if (c.type == UTF8) {
            for (int r = 0; r < rows; r++) {
                if (!nulls[r]) out[r] = text(c, c.auxAt + u32(c.at + 4 * r), c.auxAt + u32(c.at + 4 * r + 4), auxEnd);
            }
            return out;
        }
        if (c.auxLength < 8) throw malformed(c);
        long count = u32(c.auxAt);
        if (count > (c.auxLength - 8) / 4) throw malformed(c);
        long base = c.auxAt + 8 + 4 * count;
        String[] words = new String[(int) count];
        for (int r = 0; r < rows; r++) {
            if (nulls[r]) continue;
            long code = c.type == DICT16 ? Short.toUnsignedInt(view.getShort(c.at + 2 * r)) : u32(c.at + 4 * r);
            if (code >= count) throw malformed(c);
            int k = (int) code;
            if (words[k] == null) words[k] = text(c, base + u32(c.auxAt + 4 + 4 * k), base + u32(c.auxAt + 8 + 4 * k), auxEnd);
            out[r] = words[k];
        }
        return out;
    }

    private String text(Column c, long from, long to, long limit) {
        if (from > to || to > limit) throw malformed(c);
        return new String(blob, (int) from, (int) (to - from), StandardCharsets.UTF_8);
    }

    private static TeiGeodataException malformed(Column c) {
        return new TeiGeodataException.Corrupt("column " + c.name + " is malformed");
    }

    /** The column called {@code name} with one value per row, {@code null} where null. */
    public List<Object> column(String name) {
        Column c = column0(name);
        List<Object> out = new ArrayList<>(rows);
        if (isString(c.type)) {
            Collections.addAll(out, strings(name));
            return out;
        }
        boolean[] nulls = nulls(name);
        for (int r = 0; r < rows; r++) out.add(nulls[r] ? null : number(c, r));
        return out;
    }

    /** Every row as a map from column name to value, in column order. */
    public List<Map<String, Object>> records() {
        List<List<Object>> values = new ArrayList<>(layout.size());
        for (Column c : layout) values.add(column(c.name));
        List<Map<String, Object>> out = new ArrayList<>(rows);
        for (int r = 0; r < rows; r++) {
            Map<String, Object> row = new LinkedHashMap<>();
            for (int i = 0; i < layout.size(); i++) row.put(layout.get(i).name, values.get(i).get(r));
            out.add(row);
        }
        return out;
    }

    /** The rows as {@link #records} gives them. */
    @Override
    public Iterator<Map<String, Object>> iterator() {
        return records().iterator();
    }
}
