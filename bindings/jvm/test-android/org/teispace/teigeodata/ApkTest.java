package org.teispace.teigeodata;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.res.AssetFileDescriptor;
import android.os.Bundle;
import android.system.Os;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.Arrays;

/**
 * D-306 inside a real APK: a pack stored uncompressed as an asset opens in place. The asset's
 * descriptor from {@code AssetManager.openFd} names the APK at {@code /proc/self/fd/<fd>}, and
 * {@code openFileRange} on that path, the asset's offset and length reads the same pack as a
 * copy of the asset on disk. Run by {@code cargo xtask package-jvm --android-test} with
 * {@code am instrument}; the result is the bundle's {@code result}.
 */
public final class ApkTest extends Instrumentation {
    private static final String ASSET = "tz.tgd";

    @Override
    public void onCreate(Bundle arguments) {
        start();
    }

    @Override
    public void onStart() {
        Bundle out = new Bundle();
        int code;
        try {
            out.putString("result", run());
            code = Activity.RESULT_OK;
        } catch (Throwable t) {
            out.putString("result", "apk failed: " + t);
            code = Activity.RESULT_CANCELED;
        }
        finish(code, out);
    }

    private String run() throws Exception {
        File copy = new File(getContext().getCacheDir(), ASSET);
        try (InputStream in = getContext().getAssets().open(ASSET);
                OutputStream o = new FileOutputStream(copy)) {
            byte[] b = new byte[8192];
            for (int n; (n = in.read(b)) > 0; ) {
                o.write(b, 0, n);
            }
        }
        try (AssetFileDescriptor afd = getContext().getAssets().openFd(ASSET)) {
            String path = "/proc/self/fd/" + afd.getParcelFileDescriptor().getFd();
            String target = Os.readlink(path);
            long offset = afd.getStartOffset();
            long length = afd.getLength();
            check(target.endsWith(".apk"), "the descriptor names " + target);
            check(offset > 0 && length == copy.length(), "range " + offset + "+" + length);
            String version = "";
            // The descriptor's path, and the file it names (as bindings §7 shows apps).
            for (String file : new String[] {path, target}) {
                try (TeiGeodata.Engine inApk = new TeiGeodata.Engine();
                        TeiGeodata.Engine onDisk = new TeiGeodata.Engine()) {
                    inApk.openFileRange(file, offset, length);
                    onDisk.openFile(copy.toString());
                    version = inApk.tzVersion();
                    check(!version.isEmpty() && version.equals(onDisk.tzVersion()), "tz version " + version);
                    check(Arrays.equals(inApk.tzZones(), onDisk.tzZones()), "tz zones");
                }
            }
            return "apk ok: " + target + " at " + offset + "+" + length + ", tz " + version;
        }
    }

    private static void check(boolean ok, String what) {
        if (!ok) {
            throw new AssertionError(what);
        }
    }
}
