// Hand-written (not generated): how the JVM binding finds its native library (D-288).
package org.teispace.teigeodata;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.FileAlreadyExistsException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Locale;

/**
 * Loads {@code teigeodata_jvm}: from {@code java.library.path} first (Android, where the app's
 * {@code jniLibs} are on it, and a desktop caller that set it), else from the JAR's
 * {@code META-INF/native/<os>-<arch>/}, extracted once into a folder named by the library's
 * SHA-256, so JVMs share one copy, an upgrade gets its own, and a loaded DLL that Windows keeps
 * locked is never overwritten.
 */
final class NativeLoader {
    private static final String NAME = "teigeodata_jvm";

    private NativeLoader() {}

    static void load() {
        try {
            System.loadLibrary(NAME);
        } catch (UnsatisfiedLinkError notOnPath) {
            String file = System.mapLibraryName(NAME);
            String resource = "/META-INF/native/" + platform() + "/" + file;
            byte[] bytes;
            try (InputStream in = NativeLoader.class.getResourceAsStream(resource)) {
                if (in == null) {
                    UnsatisfiedLinkError e = new UnsatisfiedLinkError(
                            "teigeodata: no native library for " + platform()
                                    + " on java.library.path or in the JAR at " + resource);
                    e.addSuppressed(notOnPath);
                    throw e;
                }
                bytes = readAll(in);
            } catch (IOException e) {
                throw linkError("reading " + resource, e);
            }
            System.load(extract(bytes, file).toString());
        }
    }

    /** The library's bytes at {@code <tmp>/teigeodata-<sha256>/<file>}, written once. */
    private static Path extract(byte[] bytes, String file) {
        try {
            String hash = hex(MessageDigest.getInstance("SHA-256").digest(bytes)).substring(0, 32);
            Path dir = Paths.get(System.getProperty("java.io.tmpdir"), "teigeodata-" + hash);
            Path target = dir.resolve(file);
            if (Files.isRegularFile(target) && Files.size(target) == bytes.length) {
                return target.toAbsolutePath();
            }
            Files.createDirectories(dir);
            // Written beside it and moved into place: another JVM never loads a partial file.
            Path partial = Files.createTempFile(dir, file, ".part");
            Files.write(partial, bytes);
            try {
                Files.move(partial, target, StandardCopyOption.ATOMIC_MOVE);
            } catch (FileAlreadyExistsException | AtomicMoveNotSupportedException raced) {
                // Another JVM placed it first (or the move cannot be atomic): use what is there.
                if (!Files.isRegularFile(target)) {
                    Files.move(partial, target, StandardCopyOption.REPLACE_EXISTING);
                }
                Files.deleteIfExists(partial);
            }
            return target.toAbsolutePath();
        } catch (IOException | NoSuchAlgorithmException e) {
            throw linkError("extracting " + file, e);
        }
    }

    /** {@code <os>-<arch>} as the JAR's folders name it: linux, macos or windows; x86_64 or aarch64. */
    static String platform() {
        String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
        String arch = System.getProperty("os.arch", "").toLowerCase(Locale.ROOT);
        String o;
        if (os.startsWith("windows")) {
            o = "windows";
        } else if (os.startsWith("mac") || os.contains("darwin")) {
            o = "macos";
        } else if (os.contains("linux")) {
            o = "linux";
        } else {
            o = os.replace(' ', '-');
        }
        String a;
        if (arch.equals("amd64") || arch.equals("x86_64")) {
            a = "x86_64";
        } else if (arch.equals("aarch64") || arch.equals("arm64")) {
            a = "aarch64";
        } else {
            a = arch;
        }
        return o + "-" + a;
    }

    /** All of a stream (not {@code readAllBytes}, which Android has only from API 33). */
    private static byte[] readAll(InputStream in) throws IOException {
        java.io.ByteArrayOutputStream out = new java.io.ByteArrayOutputStream(1 << 22);
        byte[] buffer = new byte[1 << 16];
        for (int n; (n = in.read(buffer)) > 0; ) {
            out.write(buffer, 0, n);
        }
        return out.toByteArray();
    }

    private static String hex(byte[] bytes) {
        StringBuilder s = new StringBuilder(bytes.length * 2);
        for (byte b : bytes) {
            s.append(Character.forDigit((b >> 4) & 0xf, 16)).append(Character.forDigit(b & 0xf, 16));
        }
        return s.toString();
    }

    private static UnsatisfiedLinkError linkError(String what, Exception cause) {
        UnsatisfiedLinkError e = new UnsatisfiedLinkError("teigeodata: " + what + ": " + cause);
        e.initCause(cause);
        return e;
    }
}
