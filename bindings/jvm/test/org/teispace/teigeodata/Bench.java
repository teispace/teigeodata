package org.teispace.teigeodata;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;

/**
 * The binding benchmark's JVM rows (docs/02-research/benches/s4-bindings.md). With
 * TEIGEODATA_BENCH_PACK naming a pack, the fifth case runs the 1,000-row query on it.
 */
public final class Bench {
    private Bench() {}

    static double sink;

    interface Op { void run(); }

    static double best(int n, Op f) {
        double out = Double.MAX_VALUE;
        for (int r = 0; r < 5; r++) {
            long t = System.nanoTime();
            for (int i = 0; i < n; i++) f.run();
            out = Math.min(out, (double) (System.nanoTime() - t) / n);
        }
        return out;
    }

    static double latSum(byte[] blob) {
        ByteBuffer b = ByteBuffer.wrap(blob).order(ByteOrder.LITTLE_ENDIAN);
        int at = b.getInt(76), len = b.getInt(80);
        double s = 0;
        for (int i = 0; i < len; i += 8) s += b.getDouble(at + i);
        return s;
    }

    /** The descriptor of column {@code name}, or -1. */
    static int descriptor(byte[] blob, ByteBuffer b, String name) {
        for (int i = 0; i < b.getInt(8); i++) {
            int d = 16 + 40 * i;
            if (new String(blob, d, 16, StandardCharsets.US_ASCII).replace("\0", "").equals(name)) return d + 16;
        }
        return -1;
    }

    /** The 1,000-row query's decoding: every {@code name} as a Java String, and the sum of lat. */
    static double decode(byte[] blob) {
        ByteBuffer b = ByteBuffer.wrap(blob).order(ByteOrder.LITTLE_ENDIAN);
        int rows = b.getInt(4), names = descriptor(blob, b, "name"), lat = descriptor(blob, b, "lat");
        int offsets = b.getInt(names + 4), text = b.getInt(names + 12), at = b.getInt(lat + 4);
        double total = 0;
        for (int r = 0; r < rows; r++) {
            int s0 = b.getInt(offsets + 4 * r), s1 = b.getInt(offsets + 4 * r + 4);
            total += new String(blob, text + s0, s1 - s0, StandardCharsets.UTF_8).length();
        }
        for (int r = 0; r < rows; r++) total += b.getDouble(at + 8 * r);
        return total;
    }

    static void row(String c, double ns) {
        System.out.printf("%-10s %-22s %10.1f ns%n", "jvm", c, ns);
    }

    public static void main(String[] args) {
        // Warm-up so the JIT has compiled the loops before the measured rounds.
        best(200_000, () -> sink += TeiGeodata.fold("Kathmandu").length());
        row("noop", best(10_000_000, TeiGeodata::noop));
        row("fold", best(1_000_000, () -> sink += TeiGeodata.fold("Kathmandu").length()));
        row("sample_rows(1000)+sum", best(10_000, () -> sink += latSum(TeiGeodata.sampleRows(1000))));
        row("callback", best(1_000, () -> sink += TeiGeodata.progress(1000, s -> true)) / 1000);
        String pack = System.getenv("TEIGEODATA_BENCH_PACK");
        if (pack != null) {
            try (TeiGeodata.Engine engine = new TeiGeodata.Engine()) {
                engine.openFile(pack);
                byte[] first = engine.withinRadius(27.7, 85.3, 50_000, 1000);
                if (ByteBuffer.wrap(first).order(ByteOrder.LITTLE_ENDIAN).getInt(4) == 1000) {
                    best(200, () -> sink += decode(engine.withinRadius(27.7, 85.3, 50_000, 1000)));
                    row("radius(1000)+decode", best(200, () -> sink += decode(engine.withinRadius(27.7, 85.3, 50_000, 1000))));
                } else {
                    System.out.println("jvm: " + pack + ": the query does not return 1,000 rows");
                }
            }
        }
    }
}
