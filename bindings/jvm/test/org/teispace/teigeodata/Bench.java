package org.teispace.teigeodata;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;

/** The binding benchmark's JVM rows (docs/02-research/benches/s4-bindings.md). */
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
    }
}
