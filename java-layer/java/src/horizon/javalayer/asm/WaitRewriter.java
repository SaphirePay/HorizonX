package horizon.javalayer.asm;

import org.objectweb.asm.ClassReader;
import org.objectweb.asm.ClassVisitor;
import org.objectweb.asm.ClassWriter;
import org.objectweb.asm.MethodVisitor;
import org.objectweb.asm.Opcodes;

/**
 * Waits.rewrite's work, in the server's class loader (where ASM is): each call of CompletableFuture.join/get or
 * Future.get in a plugin class becomes the static horizon.javalayer.Waits method of the same name, which takes the
 * future first: the stack is the same, so nothing else changes. Only names: the layer's classes are resolved by the
 * plugin's own loader when the rewritten class runs.
 */
public final class WaitRewriter {
    private WaitRewriter() {}

    private static final String WAITS = "horizon/javalayer/Waits";
    private static final String CF = "java/util/concurrent/CompletableFuture";
    private static final String FUTURE = "java/util/concurrent/Future";

    public static byte[] rewrite(byte[] bytes) {
        ClassReader reader = new ClassReader(bytes);
        ClassWriter writer = new ClassWriter(reader, 0);
        boolean[] changed = {false};
        reader.accept(new ClassVisitor(Opcodes.ASM9, writer) {
            @Override
            public MethodVisitor visitMethod(int access, String name, String desc, String signature, String[] exceptions) {
                return new MethodVisitor(Opcodes.ASM9, super.visitMethod(access, name, desc, signature, exceptions)) {
                    @Override
                    public void visitMethodInsn(int op, String owner, String method, String d, boolean itf) {
                        String to = target(op, owner, method, d);
                        if (to != null) {
                            changed[0] = true;
                            super.visitMethodInsn(Opcodes.INVOKESTATIC, WAITS, method, to, false);
                            return;
                        }
                        super.visitMethodInsn(op, owner, method, d, itf);
                    }
                };
            }
        }, 0);
        return changed[0] ? writer.toByteArray() : bytes;
    }

    /** The Waits descriptor that stands for this call, or null to leave it. */
    private static String target(int op, String owner, String method, String desc) {
        if (op == Opcodes.INVOKEVIRTUAL && owner.equals(CF)) {
            if (method.equals("join") && desc.equals("()Ljava/lang/Object;")) return "(L" + CF + ";)Ljava/lang/Object;";
            if (method.equals("get") && desc.equals("()Ljava/lang/Object;")) return "(L" + CF + ";)Ljava/lang/Object;";
            if (method.equals("get") && desc.equals("(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;")) return "(L" + CF + ";JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;";
        }
        if (op == Opcodes.INVOKEINTERFACE && owner.equals(FUTURE)) {
            if (method.equals("get") && desc.equals("()Ljava/lang/Object;")) return "(L" + FUTURE + ";)Ljava/lang/Object;";
            if (method.equals("get") && desc.equals("(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;")) return "(L" + FUTURE + ";JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;";
        }
        return null;
    }
}
