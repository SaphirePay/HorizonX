package horizon.javalayer;

import java.util.Map;

/**
 * When the server thread has been inside Java (an event, a command, a tick) for 5 s, every Java thread's stack goes to
 * the log once: a plugin waiting on the server thread for work that itself needs the server thread shows up there.
 */
final class Watchdog {
    private Watchdog() {}

    private static volatile long enteredAt;
    private static volatile String what = "";
    private static volatile boolean told;

    static void enter(String doing) {
        what = doing;
        told = false;
        enteredAt = System.nanoTime();
    }

    static void leave() {
        enteredAt = 0;
    }

    static {
        Thread t = new Thread(() -> {
            while (true) {
                try {
                    Thread.sleep(1000);
                } catch (InterruptedException e) {
                    return;
                }
                long at = enteredAt;
                if (at != 0 && !told && System.nanoTime() - at > 5_000_000_000L) {
                    told = true;
                    StringBuilder out = new StringBuilder("[JavaLayer] the server thread has been in Java for 5 s (" + what + "); Java's threads:\n");
                    for (Map.Entry<Thread, StackTraceElement[]> e : Thread.getAllStackTraces().entrySet()) {
                        StackTraceElement[] stack = e.getValue();
                        if (stack.length == 0) continue;
                        out.append("  thread ").append(e.getKey().getName()).append(" (").append(e.getKey().getState()).append(")\n");
                        for (int i = 0; i < Math.min(stack.length, 25); i++) out.append("    at ").append(stack[i]).append('\n');
                    }
                    Native.log(2, out.toString());
                }
            }
        }, "Horizon Java watchdog");
        t.setDaemon(true);
        t.start();
    }
}
