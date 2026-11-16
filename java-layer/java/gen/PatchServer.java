import org.objectweb.asm.ClassReader;
import org.objectweb.asm.ClassVisitor;
import org.objectweb.asm.ClassWriter;
import org.objectweb.asm.Label;
import org.objectweb.asm.MethodVisitor;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.Type;

import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.Enumeration;
import java.util.Map;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;
import java.util.zip.ZipOutputStream;

/**
 * Build step: Paper's CraftItemStack in the server jar, its NMS statics taught the layer's items (which aren't
 * CraftItemStacks; libraries such as NBT-API, cloud and PacketEvents call these). Each listed static goes to the same
 * method of horizon.javalayer.nms.NmsItems, with the same arguments: "guard" ones only for a first argument that
 * isn't Paper's own CraftItemStack (Paper's code keeps those), the others always.
 *
 *     java -cp asm.jar:gen-classes PatchServer <0paper-server.jar>
 */
public class PatchServer {
    static final String CRAFT = "org/bukkit/craftbukkit/inventory/CraftItemStack";
    static final String ITEMS = "horizon/javalayer/nms/NmsItems";
    static final String B = "Lorg/bukkit/inventory/ItemStack;";
    static final String N = "Lnet/minecraft/world/item/ItemStack;";
    static final String META = "Lorg/bukkit/inventory/meta/ItemMeta;";
    static final String TYPE = "Lorg/bukkit/inventory/ItemType;";

    /** name + descriptor -> {helper name, "guard" or "always"}. */
    static final Map<String, String[]> METHODS = Map.of(
            "asNMSCopy(" + B + ")" + N, new String[]{"toNms", "guard"},
            "unwrap(" + B + ")" + N, new String[]{"toNms", "guard"},
            "asBukkitCopy(Lnet/minecraft/world/item/ItemInstance;)" + B, new String[]{"toBukkit", "always"},
            "asBukkitMirror(" + N + ")" + B, new String[]{"mirror", "always"},
            "getItemMeta(" + N + ")" + META, new String[]{"meta", "always"},
            "getItemMeta(" + N + TYPE + ")" + META, new String[]{"meta", "always"},
            "getItemMeta(" + N + TYPE + "Ljava/util/Set;)" + META, new String[]{"meta", "always"},
            "setItemMeta(" + N + META + ")Z", new String[]{"setMeta", "always"},
            "applyMetaToItem(" + N + META + ")V", new String[]{"applyMeta", "always"},
            "hasItemMeta(" + N + ")Z", new String[]{"hasMeta", "always"});

    public static void main(String[] args) throws Exception {
        Path jar = Path.of(args[0]);
        Path tmp = jar.resolveSibling(jar.getFileName() + ".patch");
        int patched = 0;
        try (ZipFile in = new ZipFile(jar.toFile()); ZipOutputStream out = new ZipOutputStream(Files.newOutputStream(tmp))) {
            for (Enumeration<? extends ZipEntry> e = in.entries(); e.hasMoreElements(); ) {
                ZipEntry entry = e.nextElement();
                byte[] bytes;
                try (InputStream s = in.getInputStream(entry)) { bytes = s.readAllBytes(); }
                if (entry.getName().equals(CRAFT + ".class")) {
                    int[] count = {0};
                    bytes = patch(bytes, count);
                    patched = count[0];
                }
                if (entry.getName().equals(ENTITY + ".class")) bytes = patchScheduler(bytes);
                out.putNextEntry(new ZipEntry(entry.getName()));
                out.write(bytes);
                out.closeEntry();
            }
        }
        Files.move(tmp, jar, StandardCopyOption.REPLACE_EXISTING);
        System.out.println("server classes: CraftItemStack's NMS statics patched for the layer's items (" + patched + " of " + METHODS.size() + " methods)" + (schedulerPatched ? ", CraftEntity.getScheduler to the layer's" : ""));
    }

    static final String ENTITY = "org/bukkit/craftbukkit/entity/CraftEntity";
    static final String SCHEDULER = "Lio/papermc/paper/threadedregions/scheduler/EntityScheduler;";
    static boolean schedulerPatched;

    /**
     * CraftEntity.getScheduler() (Paper's FoliaEntityScheduler, which only Paper's own server tick runs): the layer's
     * entity scheduler instead (horizon.javalayer.Folia.entityScheduler), on the server's scheduler, retired when the
     * entity is no longer valid. A task a plugin gave the player (DonutShards' replies) never ran.
     */
    static byte[] patchScheduler(byte[] bytes) {
        ClassReader reader = new ClassReader(bytes);
        ClassWriter writer = new ClassWriter(reader, ClassWriter.COMPUTE_MAXS);
        reader.accept(new ClassVisitor(Opcodes.ASM9, writer) {
            @Override
            public MethodVisitor visitMethod(int access, String name, String desc, String signature, String[] exceptions) {
                MethodVisitor mv = super.visitMethod(access, name, desc, signature, exceptions);
                if (!name.equals("getScheduler") || !desc.equals("()" + SCHEDULER) || (access & Opcodes.ACC_STATIC) != 0) return mv;
                schedulerPatched = true;
                return new MethodVisitor(Opcodes.ASM9, mv) {
                    @Override
                    public void visitCode() {
                        super.visitCode();
                        super.visitVarInsn(Opcodes.ALOAD, 0);
                        super.visitMethodInsn(Opcodes.INVOKESTATIC, "horizon/javalayer/Folia", "entityScheduler", "(Lorg/bukkit/entity/Entity;)" + SCHEDULER, false);
                        super.visitInsn(Opcodes.ARETURN);
                        // Paper's own body after it, never reached: the method's first frame again.
                        Label paper = new Label();
                        super.visitLabel(paper);
                        super.visitFrame(Opcodes.F_SAME, 0, null, 0, null);
                    }
                };
            }
        }, 0);
        return writer.toByteArray();
    }

    static byte[] patch(byte[] bytes, int[] count) {
        ClassReader reader = new ClassReader(bytes);
        ClassWriter writer = new ClassWriter(reader, ClassWriter.COMPUTE_MAXS);
        reader.accept(new ClassVisitor(Opcodes.ASM9, writer) {
            @Override
            public MethodVisitor visitMethod(int access, String name, String desc, String signature, String[] exceptions) {
                MethodVisitor mv = super.visitMethod(access, name, desc, signature, exceptions);
                String[] to = (access & Opcodes.ACC_STATIC) != 0 ? METHODS.get(name + desc) : null;
                if (to == null) return mv;
                count[0]++;
                Type[] params = Type.getArgumentTypes(desc);
                Type ret = Type.getReturnType(desc);
                return new MethodVisitor(Opcodes.ASM9, mv) {
                    @Override
                    public void visitCode() {
                        super.visitCode();
                        Label paper = new Label();
                        if (to[1].equals("guard")) {
                            super.visitVarInsn(Opcodes.ALOAD, 0);
                            super.visitTypeInsn(Opcodes.INSTANCEOF, CRAFT);
                            super.visitJumpInsn(Opcodes.IFNE, paper);
                        }
                        int slot = 0;
                        for (Type p : params) {
                            super.visitVarInsn(p.getOpcode(Opcodes.ILOAD), slot);
                            slot += p.getSize();
                        }
                        super.visitMethodInsn(Opcodes.INVOKESTATIC, ITEMS, to[0], desc, false);
                        super.visitInsn(ret.getOpcode(Opcodes.IRETURN));
                        // Paper's own body after (reached only past the guard): the method's first frame again.
                        super.visitLabel(paper);
                        super.visitFrame(Opcodes.F_SAME, 0, null, 0, null);
                    }
                };
            }
        }, 0);
        return writer.toByteArray();
    }
}
