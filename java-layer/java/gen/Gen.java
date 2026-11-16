import java.io.IOException;
import java.lang.reflect.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;

/**
 * Build-time writer of the Java classes that can't be proxies: `HItemStack extends ItemStack` (Paper's
 * CraftItemStack), the Paper services the API finds with ServiceLoader, and a subclass for each registry
 * element that is an abstract class (PotionEffectType...). Run with the Paper API on the class path:
 * `java -cp <api jars>;<this> Gen <out dir>`.
 */
public class Gen {
    static Path out;

    public static void main(String[] args) throws Exception {
        out = Paths.get(args[0]);
        Files.createDirectories(out.resolve("horizon/javalayer"));
        itemStack();
        craft("org.bukkit.Server", "org.bukkit.craftbukkit", "CraftServer", null, "horizon.javalayer.Bridge.serverCall(this, M[%d], %s)", false);
        craft("org.bukkit.entity.Player", "org.bukkit.craftbukkit.entity", "CraftPlayer", "org.bukkit.craftbukkit.entity.CraftHumanEntity", "horizon.javalayer.Players.invoke(this, h, M[%d], %s)", true);
        world();
        inventory();
        List<String> services = new ArrayList<>();
        for (String name : new String[]{"io.papermc.paper.InternalAPIBridge", "io.papermc.paper.ServerBuildInfo", "org.bukkit.plugin.PluginLoader",
                "io.papermc.paper.plugin.lifecycle.event.types.LifecycleEventTypeProvider", "io.papermc.paper.world.flag.FeatureFlagProvider", "io.papermc.paper.registry.event.RegistryEventTypeProvider",
                "io.papermc.paper.command.brigadier.argument.VanillaArgumentProvider",
                // The rest of what Paper's server provides by ServiceLoader (its META-INF/services).
                "io.papermc.paper.block.bed.BedEnterActionBridge", "io.papermc.paper.command.brigadier.CommandBuilderProvider",
                "io.papermc.paper.datacomponent.item.ItemComponentTypesBridge",
                "io.papermc.paper.datacomponent.item.attribute.AttributeModifierDisplayBridge", "io.papermc.paper.datacomponent.item.blocksattacks.BlocksAttacksBridge",
                "io.papermc.paper.datacomponent.item.consumable.ConsumableTypesBridge", "io.papermc.paper.registry.data.InlinedRegistryBuilderProvider",
                "io.papermc.paper.registry.data.dialog.DialogInstancesProvider", "net.kyori.adventure.text.event.ClickCallback$Provider",
                }) {
            Class<?> c;
            try {
                c = Class.forName(name, false, Gen.class.getClassLoader());
            } catch (ClassNotFoundException e) {
                System.out.println("no " + name);
                continue;
            }
            if (!c.isInterface()) continue;
            String simple = "S_" + c.getSimpleName();
            String pkg = Modifier.isPublic(c.getModifiers()) ? "horizon.javalayer" : c.getPackageName();
            service(c, simple, pkg);
            services.add(name + "=" + pkg + "." + simple);
        }
        keyedClasses();
        events();
        delegating("org.bukkit.persistence.PersistentDataContainer", "org.bukkit.craftbukkit.persistence.CraftPersistentDataContainer", "horizon.javalayer", "HPersistentDataContainer");
        metas();
        Files.writeString(out.resolve("services.txt"), String.join("\n", services) + "\n");
    }

    static String type(Class<?> c) {
        if (c.isArray()) return type(c.getComponentType()) + "[]";
        return c.getName().replace('$', '.');
    }

    static String params(Method m) {
        StringBuilder s = new StringBuilder();
        Class<?>[] p = m.getParameterTypes();
        for (int i = 0; i < p.length; i++) {
            if (i > 0) s.append(", ");
            if (m.isVarArgs() && i == p.length - 1) s.append(type(p[i].getComponentType())).append("... a").append(i);
            else s.append(type(p[i])).append(" a").append(i);
        }
        return s.toString();
    }

    static String argArray(Method m) {
        StringBuilder s = new StringBuilder("new Object[]{");
        for (int i = 0; i < m.getParameterCount(); i++) {
            if (i > 0) s.append(", ");
            s.append("a").append(i);
        }
        return s.append("}").toString();
    }

    static String classes(Method m) {
        StringBuilder s = new StringBuilder("new Class<?>[]{");
        Class<?>[] p = m.getParameterTypes();
        for (int i = 0; i < p.length; i++) {
            if (i > 0) s.append(", ");
            s.append(type(p[i])).append(".class");
        }
        return s.append("}").toString();
    }

    static String throwsClause(Method m) {
        if (m.getExceptionTypes().length == 0) return "";
        StringBuilder s = new StringBuilder(" throws ");
        for (int i = 0; i < m.getExceptionTypes().length; i++) {
            if (i > 0) s.append(", ");
            s.append(type(m.getExceptionTypes()[i]));
        }
        return s.toString();
    }

    static String returning(Method m, String call) {
        Class<?> r = m.getReturnType();
        if (r == void.class) return call + ";";
        if (r.isPrimitive()) {
            String box = switch (r.getName()) {
                case "int" -> "Integer";
                case "long" -> "Long";
                case "double" -> "Double";
                case "float" -> "Float";
                case "boolean" -> "Boolean";
                case "char" -> "Character";
                case "short" -> "Short";
                default -> "Byte";
            };
            return "Object r = " + call + "; return r == null ? (" + r.getName() + ") " + (r == boolean.class ? "false" : "0") + " : ((" + box + ") r)." + r.getName() + "Value();";
        }
        return "return (" + type(r) + ") " + call + ";";
    }

    static boolean overridable(Method m) {
        int mod = m.getModifiers();
        return Modifier.isPublic(mod) && !Modifier.isStatic(mod) && !Modifier.isFinal(mod) && !m.isSynthetic() && !m.isBridge();
    }

    /** One method per (name, parameter types): the most specific return type wins. */
    static List<Method> distinct(Collection<Method> methods) {
        Map<String, Method> best = new LinkedHashMap<>();
        for (Method m : methods) {
            String key = m.getName() + Arrays.toString(m.getParameterTypes());
            Method have = best.get(key);
            if (have == null || have.getReturnType().isAssignableFrom(m.getReturnType())) best.put(key, m);
        }
        return new ArrayList<>(best.values());
    }

    static void write(String simple, String text) throws IOException {
        Files.writeString(out.resolve("horizon/javalayer/" + simple + ".java"), text, StandardCharsets.UTF_8);
    }

    static void itemStack() throws Exception {
        Class<?> stack = Class.forName("org.bukkit.inventory.ItemStack");
        List<Method> methods = new ArrayList<>();
        for (Method m : stack.getDeclaredMethods()) if (overridable(m)) methods.add(m);
        for (String name : new String[]{"equals", "hashCode", "toString"}) {
            for (Method m : Object.class.getDeclaredMethods()) if (m.getName().equals(name) && methods.stream().noneMatch(x -> x.getName().equals(name) && Arrays.equals(x.getParameterTypes(), m.getParameterTypes()))) methods.add(m);
        }
        methods = distinct(methods);
        StringBuilder s = new StringBuilder();
        s.append("package horizon.javalayer;\n\n// Generated by Gen: the server's item behind Paper's ItemStack (not named CraftItemStack: Paper's own, with its NMS statics, stays for the libraries that look them up).\n@SuppressWarnings({\"deprecation\", \"removal\", \"unchecked\", \"rawtypes\"})\n");
        // Saved as Bukkit's ItemStack (`==: org.bukkit.inventory.ItemStack`), as CraftItemStack is on Paper.
        s.append("@org.bukkit.configuration.serialization.DelegateDeserialization(org.bukkit.inventory.ItemStack.class)\n");
        s.append("public final class HItemStack extends org.bukkit.inventory.ItemStack implements Proxies.Handled {\n");
        s.append("    private static final java.lang.reflect.Method[] M = new java.lang.reflect.Method[").append(methods.size()).append("];\n");
        s.append("    static {\n        try {\n");
        for (int i = 0; i < methods.size(); i++) {
            Method m = methods.get(i);
            s.append("            M[").append(i).append("] = ").append(type(m.getDeclaringClass())).append(".class.getDeclaredMethod(\"").append(m.getName()).append("\", ").append(classes(m)).append(");\n");
        }
        s.append("        } catch (ReflectiveOperationException e) {\n            throw new ExceptionInInitializerError(e);\n        }\n    }\n");
        s.append("    final H h;\n");
        s.append("    public HItemStack(H h) {\n        super();\n        this.h = h;\n        Proxies.CLEANER.register(this, new Proxies.Release(h.id));\n    }\n");
        s.append("    @Override public H horizonHandle() { return h; }\n");
        for (int i = 0; i < methods.size(); i++) {
            Method m = methods.get(i);
            s.append("    @Override public ").append(type(m.getReturnType())).append(" ").append(m.getName()).append("(").append(params(m)).append(")").append(throwsClause(m)).append(" {\n");
            s.append("        ").append(returning(m, "Items.invoke(this, h, M[" + i + "], " + argArray(m) + ")")).append("\n    }\n");
        }
        s.append("}\n");
        write("HItemStack", s.toString());
        System.out.println("HItemStack: " + methods.size() + " methods");
    }

    /**
     * CraftBukkit's class names for the server and players: plugins read versions from the server's package and
     * LuckPerms sets CraftHumanEntity's `perm`. Every abstract method of the interface goes to `call`.
     */
    /**
     * The layer's worlds as Paper's own CraftWorld (a subclass, made without its constructor): plugins cast a World to
     * CraftWorld and ask getHandle() for the ServerLevel (NBT-API, Pl3xMap). Every World method is the layer's
     * (NativeHandler.worldCall); getHandle() is the hollow server's level for this world.
     */
    static void world() throws Exception {
        subclass("org.bukkit.World", "org.bukkit.craftbukkit.CraftWorld", "org.bukkit.craftbukkit", "HorizonWorld", "super(null, null, null, null);",
                "    @Override public net.minecraft.server.level.ServerLevel getHandle() { return horizon.javalayer.nms.Hollow.level(this); }", "");
    }

    /**
     * The layer's own inventories (Bukkit.createInventory) as Paper's CraftInventoryCustom: plugins read its
     * `inventory` field (Duels sets the MinecraftInventory's title by reflection). Every method is the layer's.
     */
    static void inventory() throws Exception {
        subclass("org.bukkit.inventory.Inventory", "org.bukkit.craftbukkit.inventory.CraftInventoryCustom", "org.bukkit.craftbukkit.inventory", "HorizonInventory",
                "super((org.bukkit.inventory.InventoryHolder) null, 9);", "", "        horizon.javalayer.Proxies.fillCraftInventory(self);");
    }

    /** A Paper class's subclass standing for one of the layer's objects (allocated, Paper's constructor never run). */
    static void subclass(String ifaceName, String baseName, String pkg, String simple, String superCall, String extraMembers, String makeExtra) throws Exception {
        final String nl = System.lineSeparator();
        Class<?> iface = Class.forName(ifaceName);
        Class<?> base = Class.forName(baseName, false, Gen.class.getClassLoader());
        List<Method> methods = new ArrayList<>();
        // Every World method, defaults too: Paper's CraftWorld overrides some defaults with code that needs a live
        // level (rayTrace -> rayTraceBlocks); ours answer as the layer's proxies did (its answer, else the default).
        for (Method m : iface.getMethods()) if (!Modifier.isStatic(m.getModifiers())) methods.add(m);
        for (Method m : Object.class.getDeclaredMethods()) if (m.getName().equals("equals") || m.getName().equals("hashCode") || m.getName().equals("toString")) methods.add(m);
        methods = distinct(methods);
        Map<String, Method> baseMethods = new HashMap<>();
        for (Class<?> c = base; c != null && c != Object.class; c = c.getSuperclass()) {
            for (Method m : c.getDeclaredMethods()) {
                if (Modifier.isStatic(m.getModifiers()) || m.isBridge() || m.isSynthetic()) continue;
                baseMethods.putIfAbsent(m.getName() + Arrays.toString(m.getParameterTypes()), m);
            }
        }
        List<Method> kept = new ArrayList<>();
        for (Method m : methods) {
            Method b = baseMethods.get(m.getName() + Arrays.toString(m.getParameterTypes()));
            if (b != null && Modifier.isFinal(b.getModifiers())) continue;
            if (m.getName().equals("getHandle") && m.getParameterCount() == 0) continue;
            kept.add(m);
        }
        methods = kept;
        StringBuilder s = new StringBuilder();
        s.append("package ").append(pkg).append(";").append(nl).append(nl);
        s.append("// Generated by Gen: the layer's ").append(iface.getSimpleName()).append(" as Paper's ").append(base.getSimpleName()).append(".").append(nl);
        s.append("@SuppressWarnings({\"deprecation\", \"removal\", \"unchecked\", \"rawtypes\"})").append(nl);
        s.append("public final class ").append(simple).append(" extends ").append(baseName).append(" implements ").append(ifaceName).append(", horizon.javalayer.Proxies.Handled {").append(nl);
        s.append("    private static final java.lang.reflect.Method[] M = new java.lang.reflect.Method[").append(methods.size()).append("];").append(nl);
        s.append("    static {").append(nl).append("        try {").append(nl);
        for (int i = 0; i < methods.size(); i++) {
            Method m = methods.get(i);
            s.append("            M[").append(i).append("] = ").append(type(m.getDeclaringClass())).append(".class.getMethod(\"").append(m.getName()).append("\", ").append(classes(m)).append(");").append(nl);
        }
        s.append("        } catch (ReflectiveOperationException e) {").append(nl).append("            throw new ExceptionInInitializerError(e);").append(nl).append("        }").append(nl).append("    }").append(nl);
        s.append("    horizon.javalayer.H h;").append(nl);
        // Never run (Paper's constructor needs a live ServerLevel): make() allocates the object.
        s.append("    private ").append(simple).append("() { ").append(superCall).append(" }").append(nl);
        s.append("    public static ").append(simple).append(" make(horizon.javalayer.H h) {").append(nl);
        s.append("        ").append(simple).append(" self = horizon.javalayer.Craft.allocate(").append(simple).append(".class);").append(nl);
        s.append("        self.h = h;").append(nl);
        s.append("        horizon.javalayer.Proxies.CLEANER.register(self, new horizon.javalayer.Proxies.Release(h.id));").append(nl);
        if (!makeExtra.isEmpty()) s.append(makeExtra).append(nl);
        s.append("        return self;").append(nl).append("    }").append(nl);
        s.append("    @Override public horizon.javalayer.H horizonHandle() { return h; }").append(nl);
        if (!extraMembers.isEmpty()) s.append(extraMembers).append(nl);
        s.append("    private static final java.lang.invoke.MethodHandle[] DEFAULTS = new java.lang.invoke.MethodHandle[").append(methods.size()).append("];").append(nl);
        s.append("    private Object superDefault(int i, Object[] args) {").append(nl);
        s.append("        try {").append(nl);
        s.append("            java.lang.invoke.MethodHandle mh = DEFAULTS[i];").append(nl);
        s.append("            if (mh == null) {").append(nl);
        s.append("                java.lang.reflect.Method m = M[i];").append(nl);
        s.append("                mh = java.lang.invoke.MethodHandles.lookup().findSpecial(").append(ifaceName).append(".class, m.getName(), java.lang.invoke.MethodType.methodType(m.getReturnType(), m.getParameterTypes()), ").append(simple).append(".class);").append(nl);
        s.append("                DEFAULTS[i] = mh;").append(nl);
        s.append("            }").append(nl);
        s.append("            return mh.bindTo(this).invokeWithArguments(args);").append(nl);
        s.append("        } catch (RuntimeException | Error e) {").append(nl).append("            throw e;").append(nl);
        s.append("        } catch (Throwable t) {").append(nl).append("            throw new RuntimeException(t);").append(nl).append("        }").append(nl);
        s.append("    }").append(nl);
        for (int i = 0; i < methods.size(); i++) {
            Method m = methods.get(i);
            Method b = baseMethods.get(m.getName() + Arrays.toString(m.getParameterTypes()));
            Class<?> ret = b != null && m.getReturnType().isAssignableFrom(b.getReturnType()) ? b.getReturnType() : m.getReturnType();
            String call = "horizon.javalayer.NativeHandler.worldCall(this, h, M[" + i + "], " + argArray(m) + ")";
            s.append("    @Override public ").append(type(ret)).append(" ").append(m.getName()).append("(").append(params(m)).append(")").append(throwsClause(m)).append(" {").append(nl);
            if (m.isDefault()) {
                // The layer's answer, else the interface's own default (World's, never Paper's CraftWorld code): by
                // invokespecial through a MethodHandle, javac refusing World.super where CraftWorld overrides it.
                s.append("        Object res = ").append(call).append(";").append(nl);
                s.append("        if (res == horizon.javalayer.NativeHandler.USE_DEFAULT) res = superDefault(").append(i).append(", ").append(argArray(m)).append(");").append(nl);
                if (m.getReturnType() != void.class) s.append("        ").append(returning(m, "res")).append(nl);
            } else {
                s.append("        ").append(ret != m.getReturnType() ? "return (" + type(ret) + ") horizon.javalayer.Craft.narrow(" + type(ret) + ".class, " + call + ");" : returning(m, call)).append(nl);
            }
            s.append("    }").append(nl);
        }
        s.append("}").append(nl);
        Path dir = out.resolve(pkg.replace('.', '/'));
        Files.createDirectories(dir);
        Files.writeString(dir.resolve(simple + ".java"), s.toString(), StandardCharsets.UTF_8);
        System.out.println(simple + ": " + methods.size() + " methods");
    }

    static void craft(String ifaceName, String pkg, String simple, String parent, String call, boolean handled) throws Exception {
        final String nl = System.lineSeparator();
        Class<?> iface = Class.forName(ifaceName);
        List<Method> methods = new ArrayList<>();
        for (Method m : iface.getMethods()) if (Modifier.isAbstract(m.getModifiers()) && !Modifier.isStatic(m.getModifiers())) methods.add(m);
        for (Method m : Object.class.getDeclaredMethods()) if (m.getName().equals("equals") || m.getName().equals("hashCode") || m.getName().equals("toString")) methods.add(m);
        methods = distinct(methods);
        // A shadow that extends Paper's own classes (CraftPlayer: CraftLivingEntity -> CraftEntity): leave out what
        // they make final, and keep their (narrower) return types where they have the same method.
        Class<?> paperBase = handled && parent != null ? Class.forName("org.bukkit.craftbukkit.entity.CraftLivingEntity", false, Gen.class.getClassLoader()) : null;
        Map<String, Method> baseMethods = new HashMap<>();
        if (paperBase != null) {
            for (Class<?> c = paperBase; c != null && c != Object.class; c = c.getSuperclass()) {
                for (Method m : c.getDeclaredMethods()) {
                    if (Modifier.isStatic(m.getModifiers()) || m.isBridge() || m.isSynthetic()) continue;
                    baseMethods.putIfAbsent(m.getName() + Arrays.toString(m.getParameterTypes()), m);
                }
            }
            List<Method> kept = new ArrayList<>();
            for (Method m : methods) {
                Method b = baseMethods.get(m.getName() + Arrays.toString(m.getParameterTypes()));
                if (b != null && Modifier.isFinal(b.getModifiers())) continue;
                kept.add(m);
            }
            methods = kept;
        }
        StringBuilder s = new StringBuilder();
        s.append("package ").append(pkg).append(";").append(nl).append(nl);
        s.append("// Generated by Gen: ").append(ifaceName).append(" under CraftBukkit's name.").append(nl);
        s.append("@SuppressWarnings({\"deprecation\", \"removal\", \"unchecked\", \"rawtypes\"})").append(nl);
        s.append("public final class ").append(simple).append(parent == null ? "" : " extends " + parent).append(" implements ").append(type(iface)).append(handled ? ", horizon.javalayer.Proxies.Handled" : "").append(" {").append(nl);
        s.append("    private static final java.lang.reflect.Method[] M = new java.lang.reflect.Method[").append(methods.size()).append("];").append(nl);
        s.append("    static {").append(nl).append("        try {").append(nl);
        for (int i = 0; i < methods.size(); i++) {
            Method m = methods.get(i);
            s.append("            M[").append(i).append("] = ").append(type(m.getDeclaringClass())).append(".class.getMethod(\"").append(m.getName()).append("\", ").append(classes(m)).append(");").append(nl);
        }
        s.append("        } catch (ReflectiveOperationException e) {").append(nl).append("            throw new ExceptionInInitializerError(e);").append(nl).append("        }").append(nl).append("    }").append(nl);
        if (handled && paperBase != null) {
            // Never constructed (Paper's constructor needs a live vanilla entity): made by make(), fields set there.
            s.append("    horizon.javalayer.H h;").append(nl);
            s.append("    private ").append(simple).append("() { super(); }").append(nl);
            if (simple.equals("CraftPlayer")) {
                // Paper's CraftEntityTypes links CraftPlayer::new(CraftServer, ServerPlayer) when it loads (any NMS
                // entity's getBukkitEntity: GSit's seat): there for that, never run (the hollow players carry theirs).
                // A vanilla player a plugin makes itself (GSit's pose bodies, NPC plugins), not one of the server's: it
                // keeps that player, and Paper's own CraftHumanEntity code answers for it (horizon.javalayer.Npcs).
                s.append("    public CraftPlayer(org.bukkit.craftbukkit.CraftServer server, net.minecraft.server.level.ServerPlayer entity) { super(server, entity); horizon.javalayer.Npcs.init(this, server, entity); }").append(nl);
            }
            s.append("    public static ").append(simple).append(" make(horizon.javalayer.H h) {").append(nl);
            s.append("        ").append(simple).append(" self = horizon.javalayer.Craft.allocate(").append(simple).append(".class);").append(nl);
            s.append("        self.h = h;").append(nl);
            if (simple.equals("CraftPlayer")) s.append("        self.perm = new org.bukkit.permissions.PermissibleBase(self);").append(nl);
            s.append("        horizon.javalayer.Craft.entity(self, org.bukkit.entity.EntityType.PLAYER);").append(nl);
            s.append("        horizon.javalayer.Proxies.CLEANER.register(self, new horizon.javalayer.Proxies.Release(h.id));").append(nl);
            s.append("        return self;").append(nl);
            s.append("    }").append(nl);
            s.append(simple.equals("CraftPlayer")
                    ? "    @Override public net.minecraft.server.level.ServerPlayer getHandle() { return h == null ? (net.minecraft.server.level.ServerPlayer) horizon.javalayer.Npcs.handle(this) : horizon.javalayer.nms.Hollow.player(this); }"
                    : "    @Override public net.minecraft.server.level.ServerPlayer getHandle() { return horizon.javalayer.nms.Hollow.player(this); }").append(nl);
            s.append("    @Override public horizon.javalayer.H horizonHandle() { return h; }").append(nl);
        } else if (handled) {
            s.append("    final horizon.javalayer.H h;").append(nl);
            s.append("    public ").append(simple).append("(horizon.javalayer.H h) {").append(nl);
            s.append("        this.h = h;").append(nl);
            if (simple.equals("CraftPlayer")) s.append("        this.perm = new org.bukkit.permissions.PermissibleBase(this);").append(nl);
            s.append("        horizon.javalayer.Proxies.CLEANER.register(this, new horizon.javalayer.Proxies.Release(h.id));").append(nl);
            s.append("    }").append(nl);
            s.append("    @Override public horizon.javalayer.H horizonHandle() { return h; }").append(nl);
        } else {
            s.append("    public ").append(simple).append("() {}").append(nl);
        }
        // Paper's CraftPlayer sends an Adventure component as one chat message (Audience/CommandSender's default
        // would make it legacy text and split it per line): the same here, as one message.
        // CraftPlayer.addChannel/removeChannel: a channel the client listens on, as Paper takes it (Simple Voice
        // Chat registers its channels for a player so, by reflection).
        if (simple.equals("CraftPlayer") && handled) {
            s.append("    public void addChannel(String channel) { horizon.javalayer.Players.addChannel(this, channel); }").append(nl);
            s.append("    public void removeChannel(String channel) { horizon.javalayer.Players.removeChannel(this, channel); }").append(nl);
        }
        if (simple.equals("CraftPlayer") && handled) {
            s.append("    @Override public void sendMessage(net.kyori.adventure.text.Component message) { horizon.javalayer.Players.sendComponent(h, message); }").append(nl);
        }
        // Plugins reach the command map by CraftServer's field (`getDeclaredField("commandMap")`).
        if (simple.equals("CraftServer")) {
            s.append("    public final org.bukkit.command.CommandMap commandMap = horizon.javalayer.Commands.MAP;").append(nl);
            // Paper's map of the worlds by lower-case name: WorldEdit's version adapter looks the field up (getDeclaredField("worlds")).
            s.append("    private final java.util.Map<String, org.bukkit.World> worlds = new horizon.javalayer.LiveWorlds();").append(nl);
            // Paper's field for the vanilla server: PacketEvents finds it by type (Hollow.boot fills it).
            s.append("    public net.minecraft.server.dedicated.DedicatedServer console;").append(nl);
            s.append("    public net.minecraft.server.dedicated.DedicatedServer getServer() { return horizon.javalayer.nms.Hollow.server(); }").append(nl);
            // CraftServer.getHandle(): the player list (hollow; its players the online ones).
            s.append("    public net.minecraft.server.dedicated.DedicatedPlayerList getHandle() { return horizon.javalayer.nms.Hollow.playerList(); }").append(nl);
            // CraftServer.syncCommands: command frameworks (ACF, cloud, Lamp) call it after registering, to resend the
            // command tree to every player.
            s.append("    public void syncCommands() { for (org.bukkit.entity.Player p : org.bukkit.Bukkit.getOnlinePlayers()) p.updateCommands(); }").append(nl);
        }
        for (int i = 0; i < methods.size(); i++) {
            Method m = methods.get(i);
            Method b = baseMethods.get(m.getName() + Arrays.toString(m.getParameterTypes()));
            Class<?> ret = b != null && m.getReturnType().isAssignableFrom(b.getReturnType()) ? b.getReturnType() : m.getReturnType();
            s.append("    @Override public ").append(type(ret)).append(" ").append(m.getName()).append("(").append(params(m)).append(")").append(throwsClause(m)).append(" {").append(nl);
            if (ret != m.getReturnType()) System.out.println("  narrowed by Paper's base: " + m.getName() + " -> " + ret.getName());
            s.append("        ").append(ret != m.getReturnType() ? "return (" + type(ret) + ") horizon.javalayer.Craft.narrow(" + type(ret) + ".class, " + String.format(call, i, argArray(m)) + ");" : returning(m, String.format(call, i, argArray(m)))).append(nl).append("    }").append(nl);
        }
        s.append("}").append(nl);
        Path dir = out.resolve(pkg.replace('.', '/'));
        Files.createDirectories(dir);
        Files.writeString(dir.resolve(simple + ".java"), s.toString(), StandardCharsets.UTF_8);
        System.out.println(simple + ": " + methods.size() + " methods");
    }

    /**
     * A subclass of one of Paper's classes that answers an interface by handing every call to a layer object
     * (`inner`): for where Paper's own types are declared narrower than the API's (a player's PDC is Paper's
     * CraftPersistentDataContainer). Made without a constructor (Craft.allocate).
     */
    /** Paper 26.3's item meta classes (org.bukkit.craftbukkit.inventory, read from Paper's jar): name, superclass,
     *  the interfaces it declares itself. The layer's are these same classes, each method answered by NativeHandler,
     *  so getClass()/getInterfaces()/instanceof CraftMetaSkull are as on Paper. CraftMetaItem's clone() returns
     *  CraftMetaItem (a Damageable, a Repairable and an ItemMeta at once), each subclass's its own type. */
    static final String[][] META_CLASSES = {
            {"CraftMetaItem", null, "ItemMeta,Damageable,Repairable,BlockDataMeta"},
            {"CraftMetaArmor", "CraftMetaItem", "ArmorMeta"},
            {"CraftMetaColorableArmor", "CraftMetaArmor", "ColorableArmorMeta"},
            {"CraftMetaLeatherArmor", "CraftMetaItem", "LeatherArmorMeta"},
            {"CraftMetaArmorStand", "CraftMetaItem", "com.destroystokyo.paper.inventory.meta.ArmorStandMeta"},
            {"CraftMetaAxolotlBucket", "CraftMetaItem", "AxolotlBucketMeta"},
            {"CraftMetaBanner", "CraftMetaItem", "BannerMeta"},
            {"CraftMetaBlockState", "CraftMetaItem", "BlockStateMeta"},
            {"CraftMetaBook", "CraftMetaItem", "BookMeta,WritableBookMeta"},
            {"CraftMetaBookSigned", "CraftMetaItem", "BookMeta"},
            {"CraftMetaBundle", "CraftMetaItem", "BundleMeta"},
            {"CraftMetaCharge", "CraftMetaItem", "FireworkEffectMeta"},
            {"CraftMetaCompass", "CraftMetaItem", "CompassMeta"},
            {"CraftMetaCrossbow", "CraftMetaItem", "CrossbowMeta"},
            {"CraftMetaEnchantedBook", "CraftMetaItem", "EnchantmentStorageMeta"},
            {"CraftMetaEntityTag", "CraftMetaItem", ""},
            {"CraftMetaFirework", "CraftMetaItem", "FireworkMeta"},
            {"CraftMetaKnowledgeBook", "CraftMetaItem", "KnowledgeBookMeta"},
            {"CraftMetaMap", "CraftMetaItem", "MapMeta"},
            {"CraftMetaMusicInstrument", "CraftMetaItem", "MusicInstrumentMeta"},
            {"CraftMetaOminousBottle", "CraftMetaItem", "OminousBottleMeta"},
            {"CraftMetaPotion", "CraftMetaItem", "PotionMeta"},
            {"CraftMetaShield", "CraftMetaItem", "ShieldMeta,BlockStateMeta"},
            {"CraftMetaSkull", "CraftMetaItem", "SkullMeta"},
            {"CraftMetaSpawnEgg", "CraftMetaItem", "SpawnEggMeta"},
            {"CraftMetaSuspiciousStew", "CraftMetaItem", "SuspiciousStewMeta"},
            {"CraftMetaTropicalFishBucket", "CraftMetaItem", "TropicalFishBucketMeta"},
    };

    static final String META_PKG = "org.bukkit.craftbukkit.inventory";

    static Class<?> metaFace(String name) throws ClassNotFoundException {
        return Class.forName(name.contains(".") ? name : "org.bukkit.inventory.meta." + name);
    }

    static void metas() throws Exception {
        // What each generated class already declares (by name + parameters -> return type), for its subclasses.
        Map<String, Map<String, Class<?>>> declared = new HashMap<>();
        for (String[] row : META_CLASSES) {
            String simple = row[0], parent = row[1];
            List<Class<?>> faces = new ArrayList<>();
            for (String name : row[2].isEmpty() ? new String[0] : row[2].split(",")) faces.add(metaFace(name));
            Map<String, Class<?>> inherited = parent == null ? new HashMap<>() : declared.get(parent);
            // Every method of this class's own interfaces, once per signature.
            Map<String, List<Method>> bySig = new LinkedHashMap<>();
            for (Class<?> f : faces) for (Method m : f.getMethods()) {
                if (Modifier.isStatic(m.getModifiers()) || m.isDefault()) continue;
                bySig.computeIfAbsent(m.getName() + Arrays.toString(m.getParameterTypes()), k -> new ArrayList<>()).add(m);
            }
            List<Method> methods = new ArrayList<>();
            List<Boolean> own = new ArrayList<>();
            for (Map.Entry<String, List<Method>> e : bySig.entrySet()) {
                List<Method> group = e.getValue();
                Class<?> have = inherited.get(e.getKey());
                // Already answered by the superclass with a return every interface accepts: nothing to add.
                boolean covered = have != null && group.stream().allMatch(m -> m.getReturnType().isAssignableFrom(have));
                if (covered) continue;
                Method best = distinct(group).get(0);
                boolean fitsAll = have == null;
                if (fitsAll) for (Method m : group) if (!m.getReturnType().isAssignableFrom(best.getReturnType())) fitsAll = false;
                methods.add(best);
                // Clashing returns (clone()): this class's own type, which is every one of them.
                own.add(!fitsAll);
            }
            Map<String, Class<?>> mine = new HashMap<>(inherited);
            for (int i = 0; i < methods.size(); i++) {
                Method m = methods.get(i);
                // A placeholder for "this class" (the generated class isn't loadable here): Object would not do, the
                // return type only matters for the next class's `covered` test, where any interface return is checked.
                mine.put(m.getName() + Arrays.toString(m.getParameterTypes()), own.get(i) ? MetaSelf.class : m.getReturnType());
            }
            declared.put(simple, mine);

            StringBuilder s = new StringBuilder();
            s.append("package ").append(META_PKG).append(";\n\n// Generated by Gen: Paper's ").append(simple).append(", answered by the layer's NativeHandler.\n");
            s.append("@SuppressWarnings({\"deprecation\", \"removal\", \"unchecked\", \"rawtypes\"})\n");
            // Saved as Paper saves a meta (`==: ItemMeta`, CraftMetaItem's DelegateDeserialization), loaded by LegacyMeta.
            s.append("@org.bukkit.configuration.serialization.DelegateDeserialization(horizon.javalayer.LegacyMeta.class)\n");
            s.append("public class ").append(simple);
            if (parent != null) s.append(" extends ").append(parent);
            if (!faces.isEmpty()) {
                s.append(" implements ");
                for (int i = 0; i < faces.size(); i++) s.append(i > 0 ? ", " : "").append(type(faces.get(i)));
            }
            s.append(" {\n");
            int extra = parent == null ? 3 : 0;
            // Paper's own fields that plugins look up by reflection (InteractiveChat: CraftMetaSkull.profile).
            if (simple.equals("CraftMetaSkull")) s.append("    private net.minecraft.world.item.component.ResolvableProfile profile;\n    private net.minecraft.resources.Identifier noteBlockSound;\n");
            s.append("    private static final java.lang.reflect.Method[] M_").append(simple).append(" = new java.lang.reflect.Method[").append(methods.size() + extra).append("];\n");
            boolean any = methods.size() + extra > 0;
            if (any) s.append("    static {\n        try {\n");
            for (int i = 0; i < methods.size(); i++) {
                Method m = methods.get(i);
                s.append("            M_").append(simple).append("[").append(i).append("] = ").append(type(m.getDeclaringClass())).append(".class.getMethod(\"").append(m.getName()).append("\", ").append(classes(m)).append(");\n");
            }
            int eq = methods.size(), hc = eq + 1, ts = eq + 2;
            if (parent == null) {
                s.append("            M_").append(simple).append("[").append(eq).append("] = Object.class.getMethod(\"equals\", Object.class);\n");
                s.append("            M_").append(simple).append("[").append(hc).append("] = Object.class.getMethod(\"hashCode\");\n");
                s.append("            M_").append(simple).append("[").append(ts).append("] = Object.class.getMethod(\"toString\");\n");
            }
            if (any) s.append("        } catch (ReflectiveOperationException e) {\n            throw new ExceptionInInitializerError(e);\n        }\n    }\n");
            if (parent == null) {
                // The layer's handle, kept off the interface list (Paper's CraftMetaItem implements only its four).
                s.append("    protected final horizon.javalayer.NativeHandler handler;\n");
                s.append("    public ").append(simple).append("(java.lang.reflect.InvocationHandler handler) { this.handler = (horizon.javalayer.NativeHandler) handler; }\n");
                s.append("    public horizon.javalayer.H horizonHandle() { return handler.handle(); }\n");
                s.append("    protected Object call(java.lang.reflect.Method m, Object[] a) {\n");
                s.append("        try {\n            return handler.invoke(this, m, a);\n        } catch (RuntimeException | Error e) {\n            throw e;\n        } catch (Throwable t) {\n            throw new java.lang.reflect.UndeclaredThrowableException(t);\n        }\n    }\n");
                s.append("    @Override public boolean equals(Object o) { return (Boolean) call(M_").append(simple).append("[").append(eq).append("], new Object[]{o}); }\n");
                s.append("    @Override public int hashCode() { return (Integer) call(M_").append(simple).append("[").append(hc).append("], null); }\n");
                s.append("    @Override public String toString() { return String.valueOf(call(M_").append(simple).append("[").append(ts).append("], null)); }\n");
            } else {
                s.append("    public ").append(simple).append("(java.lang.reflect.InvocationHandler handler) { super(handler); }\n");
            }
            for (int i = 0; i < methods.size(); i++) {
                Method m = methods.get(i);
                String tp = m.getTypeParameters().length == 0 ? "" : "<" + String.join(", ", Arrays.stream(m.getTypeParameters()).map(t -> t.getName()).toList()) + "> ";
                String ret = own.get(i) ? simple : type(m.getReturnType());
                String args = m.getParameterCount() == 0 ? "null" : argArray(m);
                String ref = "M_" + simple + "[" + i + "]";
                s.append("    @Override public ").append(tp).append(ret).append(" ").append(m.getName()).append("(").append(params(m)).append(")").append(throwsClause(m)).append(" {\n");
                if (own.get(i)) s.append("        return (").append(simple).append(") call(").append(ref).append(", ").append(args).append(");\n    }\n");
                else s.append("        ").append(returning(m, "call(" + ref + ", " + args + ")")).append("\n    }\n");
            }
            s.append("}\n");
            Path dir = out.resolve(META_PKG.replace('.', '/'));
            Files.createDirectories(dir);
            Files.writeString(dir.resolve(simple + ".java"), s.toString(), StandardCharsets.UTF_8);
        }
        System.out.println("item metas: " + META_CLASSES.length + " classes (Paper's CraftMeta*)");
    }

    /** Stands for "the generated class itself" in Gen.metas' table of what is declared. */
    static final class MetaSelf {}

    static void delegating(String ifaceName, String baseName, String pkg, String simple) throws Exception {
        Class<?> iface = Class.forName(ifaceName);
        Class<?> base = Class.forName(baseName, false, Gen.class.getClassLoader());
        List<Method> methods = new ArrayList<>();
        for (Method m : iface.getMethods()) if (!Modifier.isStatic(m.getModifiers())) methods.add(m);
        methods = distinct(methods);
        List<Method> kept = new ArrayList<>();
        for (Method m : methods) {
            try {
                Method b = base.getMethod(m.getName(), m.getParameterTypes());
                if (Modifier.isFinal(b.getModifiers())) continue;
            } catch (NoSuchMethodException ignored) {}
            kept.add(m);
        }
        methods = kept;
        StringBuilder s = new StringBuilder();
        s.append("package ").append(pkg).append(";\n\n// Generated by Gen: ").append(ifaceName).append(" as ").append(baseName).append(", answered by a layer object.\n");
        s.append("@SuppressWarnings({\"deprecation\", \"removal\", \"unchecked\", \"rawtypes\"})\n");
        s.append("public final class ").append(simple).append(" extends ").append(type(base)).append(" {\n");
        s.append("    private static final java.lang.reflect.Method[] M = new java.lang.reflect.Method[").append(methods.size()).append("];\n");
        s.append("    static {\n        try {\n");
        for (int i = 0; i < methods.size(); i++) {
            Method m = methods.get(i);
            s.append("            M[").append(i).append("] = ").append(type(m.getDeclaringClass())).append(".class.getMethod(\"").append(m.getName()).append("\", ").append(classes(m)).append(");\n");
        }
        s.append("        } catch (ReflectiveOperationException e) {\n            throw new ExceptionInInitializerError(e);\n        }\n    }\n");
        s.append("    public Object inner;\n");
        s.append("    private ").append(simple).append("() { super((").append(type(base.getConstructors()[0].getParameterTypes().length == 1 ? base.getConstructors()[0].getParameterTypes()[0] : base.getConstructors()[1].getParameterTypes()[0])).append(") null); }\n");
        for (int i = 0; i < methods.size(); i++) {
            Method m = methods.get(i);
            String tp = m.getTypeParameters().length == 0 ? "" : "<" + String.join(", ", Arrays.stream(m.getTypeParameters()).map(t -> t.getName()).toList()) + "> ";
            s.append("    @Override public ").append(type(m.getReturnType())).append(" ").append(m.getName()).append("(").append(params(m)).append(")").append(throwsClause(m)).append(" {\n");
            s.append("        ").append(returning(m, "horizon.javalayer.Craft.delegate(inner, M[" + i + "], " + argArray(m) + ")")).append("\n    }\n");
        }
        s.append("}\n");
        Path dir = out.resolve(pkg.replace('.', '/'));
        Files.createDirectories(dir);
        Files.writeString(dir.resolve(simple + ".java"), s.toString(), StandardCharsets.UTF_8);
        System.out.println(simple + ": " + methods.size() + " methods, delegating");
    }

    static void service(Class<?> iface, String simple, String pkg) throws Exception {
        List<Method> methods = new ArrayList<>();
        for (Method m : iface.getMethods()) if (Modifier.isAbstract(m.getModifiers()) && !Modifier.isStatic(m.getModifiers())) methods.add(m);
        methods = distinct(methods);
        StringBuilder s = new StringBuilder();
        s.append("package ").append(pkg).append(";\n\n// Generated by Gen: ").append(iface.getName()).append(" for ServiceLoader.\n@SuppressWarnings({\"deprecation\", \"removal\", \"unchecked\", \"rawtypes\"})\n");
        s.append("public final class ").append(simple).append(" implements ").append(type(iface)).append(" {\n");
        s.append("    private static final java.lang.reflect.Method[] M = new java.lang.reflect.Method[").append(methods.size()).append("];\n");
        s.append("    static {\n        try {\n");
        for (int i = 0; i < methods.size(); i++) {
            Method m = methods.get(i);
            s.append("            M[").append(i).append("] = ").append(type(m.getDeclaringClass())).append(".class.getMethod(\"").append(m.getName()).append("\", ").append(classes(m)).append(");\n");
        }
        s.append("        } catch (ReflectiveOperationException e) {\n            throw new ExceptionInInitializerError(e);\n        }\n    }\n");
        s.append("    public ").append(simple).append("() {}\n");
        for (int i = 0; i < methods.size(); i++) {
            Method m = methods.get(i);
            s.append("    @Override public ").append(type(m.getReturnType())).append(" ").append(m.getName()).append("(").append(params(m)).append(")").append(throwsClause(m)).append(" {\n");
            s.append("        ").append(returning(m, "horizon.javalayer.Services.invoke(this, \"" + iface.getSimpleName() + "\", M[" + i + "], " + argArray(m) + ")")).append("\n    }\n");
        }
        s.append("}\n");
        Path dir = out.resolve(pkg.replace('.', '/'));
        Files.createDirectories(dir);
        Files.writeString(dir.resolve(simple + ".java"), s.toString(), StandardCharsets.UTF_8);
        System.out.println(simple + ": " + methods.size() + " methods");
    }

    /** Registry elements that are abstract classes (an interface is a proxy at run time). */
    static void keyedClasses() throws Exception {
        Class<?> registryKey = Class.forName("io.papermc.paper.registry.RegistryKey", false, Gen.class.getClassLoader());
        List<String> made = new ArrayList<>();
        for (Field f : registryKey.getFields()) {
            if (!Modifier.isStatic(f.getModifiers()) || !(f.getGenericType() instanceof ParameterizedType p)) continue;
            Type arg = p.getActualTypeArguments()[0];
            if (arg instanceof ParameterizedType generic) arg = generic.getRawType();
            if (!(arg instanceof Class<?> c) || c.isInterface() || c.isEnum() || !Modifier.isAbstract(c.getModifiers())) continue;
            String simple = "K_" + c.getName().substring(c.getName().lastIndexOf('.') + 1).replace('$', '_');
            if (made.contains(simple)) continue;
            Constructor<?> best = null;
            for (Constructor<?> k : c.getDeclaredConstructors()) {
                if (Modifier.isPrivate(k.getModifiers())) continue;
                if (best == null || k.getParameterCount() < best.getParameterCount()) best = k;
            }
            if (best == null) {
                System.out.println("no constructor for " + c.getName());
                continue;
            }
            List<Method> methods = new ArrayList<>();
            for (Class<?> at = c; at != null && at != Object.class; at = at.getSuperclass()) {
                for (Method m : at.getDeclaredMethods()) if (overridable(m)) methods.add(m);
            }
            for (Class<?> face : allInterfaces(c)) for (Method m : face.getMethods()) if (Modifier.isAbstract(m.getModifiers())) methods.add(m);
            for (Method m : Object.class.getDeclaredMethods()) if (m.getName().equals("equals") || m.getName().equals("hashCode") || m.getName().equals("toString")) methods.add(m);
            methods = distinct(methods);
            methods.removeIf(m -> hasDeclared(c, m));
            StringBuilder s = new StringBuilder();
            s.append("package horizon.javalayer;\n\n// Generated by Gen: a registry value of ").append(c.getName()).append(".\n@SuppressWarnings({\"deprecation\", \"removal\", \"unchecked\", \"rawtypes\"})\n");
            s.append("public final class ").append(simple).append(" extends ").append(type(c)).append(" implements Registries.Value {\n");
            s.append("    private static final class Holder {\n    static final java.lang.reflect.Method[] M = new java.lang.reflect.Method[").append(methods.size()).append("];\n");
            s.append("    static {\n        try {\n");
            for (int i = 0; i < methods.size(); i++) {
                Method m = methods.get(i);
                String how = m.getDeclaringClass().isInterface() ? "getMethod" : "getDeclaredMethod";
                s.append("            M[").append(i).append("] = ").append(type(m.getDeclaringClass())).append(".class.").append(how).append("(\"").append(m.getName()).append("\", ").append(classes(m)).append(");\n");
            }
            s.append("        } catch (ReflectiveOperationException e) {\n            throw new ExceptionInInitializerError(e);\n        }\n    }\n    }\n");
            s.append("    private final org.bukkit.NamespacedKey horizonKey;\n    private final String horizonName;\n");
            s.append("    public ").append(simple).append("(org.bukkit.NamespacedKey key, String name) {\n        super(");
            Class<?>[] kp = best.getParameterTypes();
            for (int i = 0; i < kp.length; i++) {
                if (i > 0) s.append(", ");
                Class<?> t = kp[i];
                if (t == org.bukkit.NamespacedKey.class) s.append("key");
                else if (t == String.class) s.append("name");
                else if (t == boolean.class) s.append("false");
                else if (t.isPrimitive()) s.append("(").append(t.getName()).append(") 0");
                else s.append("(").append(type(t)).append(") null");
            }
            s.append(");\n        this.horizonKey = key;\n        this.horizonName = name;\n    }\n");
            s.append("    @Override public org.bukkit.NamespacedKey horizonKey() { return horizonKey; }\n");
            s.append("    @Override public String horizonName() { return horizonName; }\n");
            for (int i = 0; i < methods.size(); i++) {
                Method m = methods.get(i);
                if (m.getName().equals("horizonKey") || m.getName().equals("horizonName")) continue;
                s.append("    @Override public ").append(type(m.getReturnType())).append(" ").append(m.getName()).append("(").append(params(m)).append(")").append(throwsClause(m)).append(" {\n");
                s.append("        ").append(returning(m, "Registries.invoke(this, Holder.M[" + i + "], " + argArray(m) + ")")).append("\n    }\n");
            }
            s.append("}\n");
            write(simple, s.toString());
            made.add(simple);
            System.out.println(simple + " (" + c.getName() + "): " + methods.size() + " methods");
        }
    }

    /** events.txt: every concrete event class of the API and the class whose HandlerList it is dispatched by. */
    static void events() throws Exception {
        String api = null;
        for (String part : System.getProperty("java.class.path").split(java.io.File.pathSeparator)) if (part.contains("paper-api")) api = part;
        List<String> lines = new ArrayList<>();
        Class<?> event = Class.forName("org.bukkit.event.Event");
        try (java.util.jar.JarFile jar = new java.util.jar.JarFile(api)) {
            for (Enumeration<java.util.jar.JarEntry> e = jar.entries(); e.hasMoreElements();) {
                String n = e.nextElement().getName();
                if (!n.endsWith(".class") || n.contains("module-info")) continue;
                Class<?> c;
                try {
                    c = Class.forName(n.substring(0, n.length() - 6).replace('/', '.'), false, Gen.class.getClassLoader());
                } catch (Throwable x) {
                    continue;
                }
                if (!event.isAssignableFrom(c) || c == event) continue;
                Class<?> owner = null;
                for (Class<?> at = c; at != null && at != event; at = at.getSuperclass()) {
                    try {
                        at.getDeclaredMethod("getHandlerList");
                        owner = at;
                        break;
                    } catch (NoSuchMethodException ignored) {}
                }
                if (owner != null) lines.add(c.getName() + " " + owner.getName());
            }
        }
        Collections.sort(lines);
        Path dir = out.resolve("resources");
        Files.createDirectories(dir);
        Files.writeString(dir.resolve("horizon-events.txt"), String.join(System.lineSeparator(), lines) + System.lineSeparator());
        System.out.println("events: " + lines.size());
    }

    static boolean hasDeclared(Class<?> c, Method m) {
        for (Class<?> at = c; at != null && at != Object.class; at = at.getSuperclass()) {
            try {
                Method d = at.getDeclaredMethod(m.getName(), m.getParameterTypes());
                if (Modifier.isFinal(d.getModifiers())) return true;
            } catch (NoSuchMethodException ignored) {}
        }
        return false;
    }

    static Set<Class<?>> allInterfaces(Class<?> c) {
        Set<Class<?>> out = new LinkedHashSet<>();
        Deque<Class<?>> todo = new ArrayDeque<>();
        for (Class<?> at = c; at != null; at = at.getSuperclass()) todo.addAll(Arrays.asList(at.getInterfaces()));
        while (!todo.isEmpty()) {
            Class<?> i = todo.pop();
            if (out.add(i)) todo.addAll(Arrays.asList(i.getInterfaces()));
        }
        return out;
    }
}
