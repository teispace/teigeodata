package org.teispace.teigeodata;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Parity: the JVM binding gives the results bindings/parity.json expects (checked there against
 * Rust). Run: {@code java ParityTest <path to parity.json>}; exits non-zero on the first failure.
 */
public final class ParityTest {
    private ParityTest() {}

    public static void main(String[] args) throws Exception {
        Map<?, ?> p = (Map<?, ?>) new Json(Files.readString(Path.of(args[0]))).value();
        check(TeiGeodata.version().equals(p.get("version")), "version");
        TeiGeodata.noop();
        for (Object c : (List<?>) p.get("fold")) {
            List<?> pair = (List<?>) c;
            String got = TeiGeodata.fold((String) pair.get(0));
            check(got.equals(pair.get(1)), "fold " + pair.get(0) + " -> " + got);
        }
        Map<?, ?> s = (Map<?, ?>) p.get("sample_rows");
        byte[] blob = TeiGeodata.sampleRows(((Number) s.get("rows")).longValue());
        check(blob.length == ((Number) s.get("bytes")).intValue(), "blob length");
        ByteBuffer lat = column(blob, "lat");
        double latSum = 0;
        while (lat.hasRemaining()) latSum += lat.getDouble();
        check(latSum == ((Number) s.get("lat_sum")).doubleValue(), "lat sum " + latSum);
        ByteBuffer e = column(blob, "elevation");
        double eSum = 0;
        while (e.hasRemaining()) eSum += e.getFloat();
        check(eSum == ((Number) s.get("elevation_sum")).doubleValue(), "elevation sum " + eSum);
        check(TeiGeodata.SAMPLE_ROWS_MAX == ((Number) p.get("sample_rows_max")).longValue(), "max");

        expect(TeiGeodataException.InvalidArgument.class, () -> TeiGeodata.sampleRows(TeiGeodata.SAMPLE_ROWS_MAX + 1), "at most");
        expect(TeiGeodataException.InvalidArgument.class, () -> TeiGeodata.sampleRows(-1), "outside its range");
        expect(TeiGeodataException.InvalidArgument.class, () -> TeiGeodata.sampleRows(1L << 33), "outside its range");

        Map<?, ?> dm = (Map<?, ?>) p.get("distance_m");
        List<?> da = (List<?>) dm.get("args");
        double d = TeiGeodata.distanceM(n(da, 0), n(da, 1), n(da, 2), n(da, 3));
        check(Math.abs(d - ((Number) dm.get("metres")).doubleValue()) < 1e-6, "distance " + d);
        List<?> bad = (List<?>) dm.get("out_of_range");
        expect(TeiGeodataException.InvalidArgument.class, () -> TeiGeodata.distanceM(n(bad, 0), n(bad, 1), n(bad, 2), n(bad, 3)), "within");

        List<Long> seen = new ArrayList<>();
        check(TeiGeodata.progress(3, step -> seen.add(step)) == 3 && seen.equals(List.of(1L, 2L, 3L)), "progress");
        Map<?, ?> g = (Map<?, ?>) p.get("progress");
        long after = ((Number) g.get("cancel_after")).longValue();
        TeiGeodataException ex = expect(TeiGeodataException.Cancelled.class,
                () -> TeiGeodata.progress(((Number) g.get("steps")).longValue(), step -> step < after),
                (String) g.get("cancelled_message"));
        check(ex.code() == ((Number) ((Map<?, ?>) p.get("error_codes")).get("Cancelled")).intValue(), "code");
        IllegalStateException boom = new IllegalStateException("boom");
        try {
            TeiGeodata.progress(5, step -> { throw boom; });
            check(false, "callback exception was swallowed");
        } catch (IllegalStateException caught) {
            check(caught == boom, "callback exception is rethrown unchanged");
        }
        System.out.println("jvm parity ok");
    }

    /** A fixed-width column of a v0 blob, viewed in place (no copy). */
    static ByteBuffer column(byte[] blob, String name) {
        ByteBuffer b = ByteBuffer.wrap(blob).order(ByteOrder.LITTLE_ENDIAN);
        check(new String(blob, 0, 4, StandardCharsets.US_ASCII).equals("TGB1"), "magic");
        int columns = b.getInt(8);
        for (int i = 0; i < columns; i++) {
            int d = 16 + 40 * i;
            String n = new String(blob, d, 16, StandardCharsets.UTF_8).replace("\0", "");
            if (n.equals(name)) {
                int at = b.getInt(d + 20), len = b.getInt(d + 24);
                return b.slice(at, len).order(ByteOrder.LITTLE_ENDIAN);
            }
        }
        throw new AssertionError("no column " + name);
    }

    static double n(List<?> l, int i) {
        return ((Number) l.get(i)).doubleValue();
    }

    static void check(boolean ok, String what) {
        if (!ok) throw new AssertionError(what);
    }

    static <T extends TeiGeodataException> T expect(Class<T> type, Runnable call, String message) {
        try {
            call.run();
        } catch (TeiGeodataException e) {
            check(type.isInstance(e), "expected " + type.getSimpleName() + ", got " + e);
            check(e.getMessage().contains(message), "message: " + e.getMessage());
            return type.cast(e);
        }
        throw new AssertionError("no exception, expected " + type.getSimpleName());
    }

    /** A minimal JSON reader: enough for parity.json (objects, arrays, strings, numbers). */
    static final class Json {
        private final String s;
        private int i;

        Json(String s) {
            this.s = s;
        }

        Object value() {
            skip();
            char c = s.charAt(i);
            if (c == '{') {
                Map<String, Object> m = new LinkedHashMap<>();
                i++;
                skip();
                if (s.charAt(i) == '}') { i++; return m; }
                while (true) {
                    skip();
                    String k = string();
                    skip();
                    i++; // ':'
                    m.put(k, value());
                    skip();
                    if (s.charAt(i++) == '}') return m;
                }
            }
            if (c == '[') {
                List<Object> l = new ArrayList<>();
                i++;
                skip();
                if (s.charAt(i) == ']') { i++; return l; }
                while (true) {
                    l.add(value());
                    skip();
                    if (s.charAt(i++) == ']') return l;
                }
            }
            if (c == '"') return string();
            int start = i;
            while (i < s.length() && "+-.eE0123456789".indexOf(s.charAt(i)) >= 0) i++;
            return Double.parseDouble(s.substring(start, i));
        }

        private String string() {
            StringBuilder b = new StringBuilder();
            i++; // opening quote
            while (true) {
                char c = s.charAt(i++);
                if (c == '"') return b.toString();
                if (c == '\\') {
                    char e = s.charAt(i++);
                    if (e == 'u') { b.append((char) Integer.parseInt(s.substring(i, i + 4), 16)); i += 4; }
                    else b.append(switch (e) { case 'n' -> '\n'; case 't' -> '\t'; default -> e; });
                } else b.append(c);
            }
        }

        private void skip() {
            while (i < s.length() && Character.isWhitespace(s.charAt(i))) i++;
        }
    }
}
