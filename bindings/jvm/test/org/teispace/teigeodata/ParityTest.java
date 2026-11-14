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
        engine((Map<?, ?>) p.get("engine"), Path.of(args[0]).toAbsolutePath().getParent());
        System.out.println("jvm parity ok");
    }

    /** The engine handle on the fixture pack (parity.json, "engine"). */
    static void engine(Map<?, ?> p, Path dir) throws java.io.IOException {
        TeiGeodata.Engine engine = new TeiGeodata.Engine();
        {
            check(engine.openFile(dir.resolve((String) p.get("fixture")).toString()) == 0, "open");
            Map<?, ?> g = (Map<?, ?>) p.get("get");
            byte[] got = engine.get((String) g.get("id"));
            check(strings(got, "name").equals(List.of(g.get("name"))), "get name");
            check(strings(got, "kind").equals(List.of(g.get("kind"))), "get kind");
            check(column(got, "lat").getDouble() == ((Number) g.get("lat")).doubleValue(), "get lat");
            Map<?, ?> q = (Map<?, ?>) p.get("search");
            byte[] hits = engine.search((String) q.get("query"), ((Number) q.get("limit")).longValue());
            check(strings(hits, "id").get(0).equals(q.get("first_id")), "search");
            Map<?, ?> r = (Map<?, ?>) p.get("reverse");
            byte[] near = engine.reverse(((Number) r.get("lat")).doubleValue(), ((Number) r.get("lon")).doubleValue(),
                    ((Number) r.get("limit")).longValue());
            check(strings(near, "id").equals(r.get("ids")), "reverse " + strings(near, "id"));
            double m = column(near, "meters").getDouble();
            check(Math.abs(m - ((Number) r.get("first_meters")).doubleValue()) < 0.1, "meters " + m);
            Map<?, ?> a = (Map<?, ?>) p.get("admin_at");
            byte[] units = engine.adminAt(((Number) a.get("lat")).doubleValue(), ((Number) a.get("lon")).doubleValue());
            check(strings(units, "id").equals(a.get("ids")), "admin_at");
            Map<?, ?> pa = (Map<?, ?>) p.get("parent"), an = (Map<?, ?>) p.get("ancestors"), ch = (Map<?, ?>) p.get("children");
            check(strings(engine.parent((String) pa.get("id")), "id").equals(pa.get("ids")), "parent");
            check(strings(engine.ancestors((String) an.get("id")), "id").equals(an.get("ids")), "ancestors");
            check(strings(engine.children((String) ch.get("id")), "id").equals(ch.get("ids")), "children");
            Map<?, ?> nb = (Map<?, ?>) p.get("nearby");
            check(strings(engine.nearby((String) nb.get("id"), ((Number) nb.get("limit")).longValue()), "id").equals(nb.get("ids")), "nearby");
            Map<?, ?> w = (Map<?, ?>) p.get("within_radius");
            double wl = ((Number) w.get("lat")).doubleValue(), wo = ((Number) w.get("lon")).doubleValue();
            check(strings(engine.withinRadius(wl, wo, ((Number) w.get("meters")).doubleValue(),
                    ((Number) w.get("limit")).longValue()), "id").equals(w.get("ids")), "within_radius");
            Map<?, ?> tz = (Map<?, ?>) p.get("tz_at");
            check(strings(engine.tzAt(((Number) tz.get("lat")).doubleValue(), ((Number) tz.get("lon")).doubleValue()), "tz")
                    .equals(List.of(tz.get("tz"))), "tz_at");
            Map<?, ?> iso = (Map<?, ?>) p.get("by_iso");
            check(strings(engine.byIso((String) iso.get("code")), "id").equals(List.of(iso.get("id"))), "by_iso");
            expect(TeiGeodataException.InvalidArgument.class, () -> engine.byIso((String) p.get("bad_iso")), "ISO 3166");
            expect(TeiGeodataException.InvalidArgument.class, () -> engine.withinRadius(wl, wo, -1, 1), "radius");
            Map<?, ?> sw = (Map<?, ?>) p.get("search_with");
            TeiGeodata.SearchOptions focus = TeiGeodata.SearchOptions.DEFAULTS
                    .withFocusLat(((Number) sw.get("focus_lat")).doubleValue())
                    .withFocusLon(((Number) sw.get("focus_lon")).doubleValue())
                    .withFocusScaleM(((Number) sw.get("focus_scale_m")).doubleValue());
            long swLimit = ((Number) sw.get("limit")).longValue();
            check(strings(engine.searchWith((String) sw.get("query"), swLimit, focus), "id").equals(sw.get("ids")), "search_with");
            check(!strings(engine.searchWith((String) sw.get("query"), swLimit, null), "id").equals(sw.get("ids")), "search_with defaults");
            expect(TeiGeodataException.InvalidArgument.class,
                    () -> engine.searchWith("x", 1, TeiGeodata.SearchOptions.DEFAULTS.withFocusLat(1.0)), "focus");
            Map<?, ?> ow = (Map<?, ?>) p.get("open_with");
            Path fixture = dir.resolve((String) p.get("fixture"));
            byte[] bad = Files.readAllBytes(fixture);
            bad[((Number) ow.get("corrupt_at")).intValue()] ^= (byte) 0xff;
            try (TeiGeodata.Engine scratch = new TeiGeodata.Engine()) {
                TeiGeodata.OpenOptions eager = TeiGeodata.OpenOptions.DEFAULTS.withVerify(TeiGeodata.Verify.EAGER);
                expect(TeiGeodataException.Corrupt.class, () -> scratch.openBytesWith(bad, eager), "corrupted");
                check(scratch.openBytesWith(bad, null) == 0, "lazy");
                TeiGeodata.OpenOptions every = eager.withLoad(TeiGeodata.Load.READ).withPriority(2).withOutlineLevel(1L);
                check(scratch.openFileWith(fixture.toString(), every) == 1, "open_file_with");
                check(scratch.openFileWith(fixture.toString(), new TeiGeodata.OpenOptions(0, null, null, null, 0, null, null)) == 2, "null enums and text");
                long deep = ((Number) ow.get("bad_outline_level")).longValue();
                expect(TeiGeodataException.InvalidArgument.class,
                        () -> scratch.openFileWith(fixture.toString(), TeiGeodata.OpenOptions.DEFAULTS.withOutlineLevel(deep)), "outline level");
                TeiGeodata.OpenOptions only = TeiGeodata.OpenOptions.DEFAULTS
                        .withComponents((String) ow.get("components")).withLanguages((String) ow.get("languages"));
                try (TeiGeodata.Engine points = new TeiGeodata.Engine()) {
                    points.openFileWith(fixture.toString(), only);
                    check(strings(points.get((String) ((Map<?, ?>) p.get("get")).get("id")), "id").size() == 1, "points get");
                    expect(TeiGeodataException.MissingComponent.class, () -> points.search("Synthland", 1), "search");
                    expect(TeiGeodataException.InvalidArgument.class,
                            () -> points.openFileWith(fixture.toString(), TeiGeodata.OpenOptions.DEFAULTS.withComponents((String) ow.get("bad_components"))), "pointy");
                }
            }
            Map<?, ?> pl = (Map<?, ?>) p.get("postal_lookup");
            byte[] hit = engine.postalLookup((String) pl.get("code"), (String) pl.get("scheme"));
            check(strings(hit, "postal").equals(pl.get("postal")) && strings(hit, "id").equals(pl.get("ids")), "postal_lookup");
            Map<?, ?> pp = (Map<?, ?>) p.get("postal_prefix");
            check(strings(engine.postalPrefix((String) pp.get("input"), (String) pp.get("scheme"),
                    ((Number) pp.get("limit")).longValue()), "postal").equals(pp.get("postal")), "postal_prefix");
            Map<?, ?> pt = (Map<?, ?>) p.get("postal_at");
            byte[] at = engine.postalAt(((Number) pt.get("lat")).doubleValue(), ((Number) pt.get("lon")).doubleValue());
            check(strings(at, "postal").equals(pt.get("postal")) && strings(at, "code_kind").equals(pt.get("code_kind")), "postal_at");
            Map<?, ?> po = (Map<?, ?>) p.get("postal_codes_of");
            check(strings(engine.postalCodesOf((String) po.get("id")), "postal").equals(po.get("postal")), "postal_codes_of");
            expect(TeiGeodataException.InvalidArgument.class, () -> engine.get((String) p.get("bad_id")), "not a place ID");
            expect(TeiGeodataException.Io.class, () -> engine.openFile("/nonexistent.tgd"), "");
            expect(TeiGeodataException.Corrupt.class, () -> engine.openBytes("not a pack".getBytes(StandardCharsets.UTF_8)), "");
            check(engine.layers() == 1, "layers");
            Map<?, ?> h = (Map<?, ?>) p.get("elevation_at");
            check(engine.openFile(dir.resolve((String) h.get("fixture")).toString()) == 1, "open dem");
            byte[] height = engine.elevationAt(((Number) h.get("lat")).doubleValue(), ((Number) h.get("lon")).doubleValue());
            check(column(height, "meters").getDouble() == ((Number) h.get("meters")).doubleValue(), "meters");
            check(strings(height, "source").equals(List.of(h.get("source"))), "source");
            check(engine.remove(1) && !engine.remove(1) && engine.layers() == 1, "remove");
            engine.close();
            engine.close(); // idempotent
            try {
                engine.layers();
                check(false, "a closed engine answered");
            } catch (IllegalStateException expected) {
                // closed
            }
        }
    }

    /** A string column (UTF-8 or a dictionary) of a v1 blob, null rows as null. */
    static List<String> strings(byte[] blob, String name) {
        ByteBuffer b = ByteBuffer.wrap(blob).order(ByteOrder.LITTLE_ENDIAN);
        int rows = b.getInt(4), columns = b.getInt(8);
        for (int i = 0; i < columns; i++) {
            int d = 16 + 40 * i;
            if (!new String(blob, d, 16, StandardCharsets.UTF_8).replace("\0", "").equals(name)) continue;
            int type = b.getInt(d + 16), at = b.getInt(d + 20), aux = b.getInt(d + 28), nulls = b.getInt(d + 36);
            List<String> out = new ArrayList<>();
            for (int r = 0; r < rows; r++) {
                if (nulls != 0 && ((blob[nulls + (r >> 3)] >> (r & 7)) & 1) == 0) {
                    out.add(null);
                } else if (type == 7) {
                    int s0 = b.getInt(at + 4 * r), s1 = b.getInt(at + 4 * r + 4);
                    out.add(new String(blob, aux + s0, s1 - s0, StandardCharsets.UTF_8));
                } else {
                    int code = type == 11 ? Short.toUnsignedInt(b.getShort(at + 2 * r)) : b.getInt(at + 4 * r);
                    int count = b.getInt(aux), base = aux + 8 + 4 * count;
                    int s0 = b.getInt(aux + 4 + 4 * code), s1 = b.getInt(aux + 8 + 4 * code);
                    out.add(new String(blob, base + s0, s1 - s0, StandardCharsets.UTF_8));
                }
            }
            return out;
        }
        throw new AssertionError("no column " + name);
    }

    /** A fixed-width column of a blob, viewed in place (no copy). */
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
