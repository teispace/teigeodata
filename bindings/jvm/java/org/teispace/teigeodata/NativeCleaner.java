// Hand-written (not generated): frees native handles whose owner became unreachable (D-288).
package org.teispace.teigeodata;

import java.lang.ref.PhantomReference;
import java.lang.ref.ReferenceQueue;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicReference;

/**
 * What {@code java.lang.ref.Cleaner} does, on every JVM the binding supports: Android has
 * {@code Cleaner} only from API 33 and the library supports API 24. Each owner gets a phantom
 * reference; one daemon thread runs a reference's action once its owner is unreachable, and
 * {@link Cleanable#clean} runs it at once instead. Either way it runs exactly once.
 */
final class NativeCleaner {
    private static final ReferenceQueue<Object> QUEUE = new ReferenceQueue<>();

    /** The references not yet run: a phantom reference must itself stay reachable. */
    private static final Set<Cleanable> PENDING = ConcurrentHashMap.newKeySet();

    static {
        Thread drain = new Thread(NativeCleaner::drain, "teigeodata-cleaner");
        drain.setDaemon(true);
        drain.start();
    }

    private NativeCleaner() {}

    /** Runs {@code action} once {@code owner} is unreachable, unless cleaned first. */
    static Cleanable register(Object owner, Runnable action) {
        Cleanable c = new Cleanable(owner, action);
        PENDING.add(c);
        return c;
    }

    /** One owner's action. */
    static final class Cleanable extends PhantomReference<Object> {
        private final AtomicReference<Runnable> action;

        private Cleanable(Object owner, Runnable action) {
            super(owner, QUEUE);
            this.action = new AtomicReference<>(action);
        }

        /** Runs the action now if it has not run; later calls, and the collector's, do nothing. */
        void clean() {
            Runnable a = action.getAndSet(null);
            if (a != null) {
                PENDING.remove(this);
                clear();
                a.run();
            }
        }
    }

    private static void drain() {
        while (true) {
            try {
                ((Cleanable) QUEUE.remove()).clean();
            } catch (InterruptedException e) {
                // A daemon thread with nothing to stop for: keep draining.
            } catch (RuntimeException | Error e) {
                // One failed free must not stop the others.
            }
        }
    }
}
