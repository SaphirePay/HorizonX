package horizon.javalayer;

import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.flattener.ComponentFlattener;
import org.bukkit.Material;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.time.Instant;
import java.util.*;

/** Paper's own services (found with ServiceLoader): the parts plugins reach, answered here. */
public final class Services {
    private Services() {}

    static final String MC = "26.3";

    public static Object invoke(Object self, String service, Method m, Object[] args) {
        String name = m.getName();
        int n = args == null ? 0 : args.length;
        switch (service) {
            case "InternalAPIBridge":
                switch (name) {
                    case "createEmptyStack":
                        return Items.create(Material.AIR, 1);
                    case "deserializeItem":
                        // ItemStack.deserializeBytes: the game's binary item, as serializeAsBytes wrote it.
                        if (n == 1 && args[0] instanceof byte[] bytes) {
                            Object raw = Native.tryStatic("ItemStack", "deserializeBytes", Conv.args(new Object[]{bytes}));
                            if (raw == Native.MISSING) throw Report.cant("ItemStack.deserializeBytes");
                            return Conv.fromNative(raw, org.bukkit.inventory.ItemStack.class);
                        }
                        break;
                    case "createPluginLifecycleEventManager":
                        return Brigadier.lifecycleManager(m.getReturnType(), (org.bukkit.plugin.Plugin) args[0]);
                    case "createDamageSourceBuilder":
                        return DamageSources.builder(m.getReturnType(), (org.bukkit.damage.DamageType) args[0]);
                    case "componentFlattener":
                        return Translations.FLATTENER;
                    case "restricted":
                        return args[0];
                    case "constructLegacyCustomBiome":
                        return Registries.value(org.bukkit.block.Biome.class, org.bukkit.NamespacedKey.minecraft("custom"));
                    case "legacyGameRuleBridge":
                        // GameRule.<clinit>: an old rule (boolean where today it is an int, or the reverse) over today's.
                        if (n == 4) return LegacyRule.of((org.bukkit.GameRule) args[0], (java.util.function.Function) args[1], (java.util.function.Function) args[2], (Class) args[3]);
                        break;
                    case "getTranslationKey":
                        if (n == 1 && args[0] instanceof org.bukkit.entity.EntityType type) return "entity.minecraft." + type.getKey().getKey();
                        break;
                    default:
                        break;
                }
                break;
            case "ServerBuildInfo":
                switch (name) {
                    case "brandId": return Key.key("papermc", "paper");
                    // Paper's own brand only (ServerBuildInfoImpl): papermc:folia is false, as on Paper.
                    case "isBrandCompatible": return n == 1 && Key.key("papermc", "paper").equals(args[0]);
                    case "brandName": return "Paper";
                    case "minecraftVersionId":
                    case "minecraftVersionName": return MC;
                    case "buildNumber": return OptionalInt.of(235);
                    case "buildTime": return Instant.EPOCH;
                    case "gitBranch":
                    case "gitCommit": return Optional.empty();
                    case "asString": return "Horizon (Paper " + MC + " API)";
                    default: break;
                }
                break;
            case "VanillaArgumentProvider":
                return Brigadier.argument(name, args);
            case "LifecycleEventTypeProvider":
                if ((name.equals("prioritized") || name.equals("monitor")) && n >= 1) return Brigadier.eventType(m.getReturnType(), String.valueOf(args[0]));
                if (m.getReturnType().isInterface()) return inert(m.getReturnType(), service + "." + name);
                break;
            case "FeatureFlagProvider":
            case "RegistryEventTypeProvider":
                if (m.getReturnType().isInterface()) return inert(m.getReturnType(), service + "." + name);
                break;
            default:
                break;
        }
        // Paper's bridges (ItemComponentTypesBridge, ConsumableTypesBridge, the dialog and registry builders...) are
        // what the API's static factories call: CustomModelData.customModelData() -> bridge().customModelData().
        // The server has those factories by their API names: asked as `Owner.name`, owner from the return type.
        if (service.equals("Provider") && name.equals("create") && n == 2) return Callbacks.make(args[0], args[1]);
        if (service.equals("InlinedRegistryBuilderProvider") && name.equals("createDialog") && n == 1) return Dialogs.create(args[0]);
        if (service.equals("ItemComponentTypesBridge") && name.equals("lore") && n == 0) return LoreBuilder.make();
        if (service.equals("ItemComponentTypesBridge") && name.equals("enchantments") && n == 0) return LoreBuilder.enchantments();
        if (service.equals("InlinedRegistryBuilderProvider") && (name.equals("createTrimMaterial") || name.equals("createTrimPattern")) && n == 1) return Inline.create(m, args[0]);
        if (service.equals("DialogInstancesProvider") && name.equals("multilineOptions") && n == 2) return Inline.multiline((Integer) args[0], (Integer) args[1]);
        if (service.equals("BedEnterActionBridge") && name.startsWith("create") && n == 0) return bedProblem(name.substring(6).replace("Problem", ""));
        if (service.equals("DialogInstancesProvider") && name.equals("register") && n == 2) return Callbacks.dialogAction(args[0], args[1]);
        // Paper's builders for these two; the server's DialogType takes the whole thing at build().
        if (service.equals("DialogInstancesProvider") && (name.equals("multiAction") || name.equals("dialogList")) && n == 1) return dialogTypeBuilder(m.getReturnType(), name, args[0]);
        if (service.equals("DialogInstancesProvider") && name.equals("notice") && n == 0) {
            // DialogType.notice(): vanilla's default OK button.
            Object ok = Native.tryStatic("ActionButton", "create", Conv.args(new Object[]{net.kyori.adventure.text.Component.translatable("gui.ok"), null, 150, null}));
            Object raw = ok == Native.MISSING ? ok : Native.tryStatic("DialogType", "notice", new Object[]{ok});
            if (raw != Native.MISSING) return Conv.fromNative(raw, m.getGenericReturnType());
        }
        Object[] nargs = Conv.args(args);
        String[] alias = BridgeAliases.A.get(service + "." + name + "/" + n);
        if (alias != null) {
            int want = Integer.parseInt(alias[2]);
            Object[] call = want > nargs.length ? Arrays.copyOf(nargs, want) : nargs;
            Object raw = Native.tryStatic(alias[0], alias[1], call);
            if (raw == Native.MISSING) {
                // Keys as text (the server's factories take ids as strings).
                Object[] text = call.clone();
                for (int i = 0; i < text.length; i++) if (text[i] instanceof Pack p && p.tag == Pack.KEY) text[i] = p.s;
                raw = Native.tryStatic(alias[0], alias[1], text);
            }
            if (raw != Native.MISSING) return Conv.fromNative(raw, m.getGenericReturnType());
        }
        Class<?> r = m.getReturnType();
        for (Class<?> owner = r; owner != null; owner = owner.getEnclosingClass()) {
            Object raw = Native.tryStatic(owner.getSimpleName(), name, nargs);
            // Bridge names often carry the owner in front: dialogBaseBuilder -> DialogBase.builder.
            String prefix = Character.toLowerCase(owner.getSimpleName().charAt(0)) + owner.getSimpleName().substring(1);
            if (raw == Native.MISSING && name.startsWith(prefix) && name.length() > prefix.length()) {
                String rest = name.substring(prefix.length());
                raw = Native.tryStatic(owner.getSimpleName(), Character.toLowerCase(rest.charAt(0)) + rest.substring(1), nargs);
            }
            if (raw != Native.MISSING) return Conv.fromNative(raw, m.getGenericReturnType());
        }
        NativeHandler.missing(service + "." + name + "/" + n);
        throw Report.cant(service + "." + name);
    }

    /** MultiActionType.Builder / DialogListType.Builder: columns (2), button width (150), exit action, as Paper's. */
    static Object dialogTypeBuilder(Class<?> type, String kind, Object contents) {
        Object[] state = {2, 150, null}; // columns, button width, exit action
        return Proxy.newProxyInstance(Services.class.getClassLoader(), new Class<?>[]{type}, (proxy, m, args) -> {
            switch (m.getName()) {
                case "columns": state[0] = args[0]; return proxy;
                case "buttonWidth": state[1] = args[0]; return proxy;
                case "exitAction": state[2] = args[0]; return proxy;
                case "build": {
                    Object[] call = kind.equals("multiAction") ? new Object[]{contents, state[2], state[0]} : new Object[]{contents, state[2], state[0], state[1]};
                    Object raw = Native.tryStatic("DialogType", kind, Conv.args(call));
                    if (raw == Native.MISSING) throw Report.cant("DialogType." + kind);
                    return Conv.fromNative(raw, m.getGenericReturnType());
                }
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
                case "toString": return kind + " builder";
                default: return m.getReturnType().isInstance(proxy) ? proxy : null;
            }
        });
    }

    /** An object that accepts everything and does nothing: a part of Paper the server has no use for yet. */
    /** Paper's BedEnterProblemImpl: the game's BedSleepingProblem and its message (none for an explosion or other). */
    static Object bedProblem(String what) {
        String key = switch (what) {
            case "TooFarAway" -> "block.minecraft.bed.too_far_away";
            case "Obstructed" -> "block.minecraft.bed.obstructed";
            case "NotSafe" -> "block.minecraft.bed.not_safe";
            default -> null;
        };
        net.kyori.adventure.text.Component message = key == null ? null : net.kyori.adventure.text.Component.translatable(key);
        String text = "BedEnterProblemImpl[vanillaProblem=BedSleepingProblem[message=" + (key == null ? "null" : "translation{key='" + key + "', args=[]}") + "], errorMessage=" + message + "]";
        return Proxy.newProxyInstance(Services.class.getClassLoader(), new Class<?>[]{io.papermc.paper.block.bed.BedEnterProblem.class}, (proxy, m, args) -> switch (m.getName()) {
            case "errorMessage" -> message;
            case "hashCode" -> System.identityHashCode(proxy);
            case "equals" -> proxy == args[0];
            default -> text;
        });
    }

    static Object inert(Class<?> type, String what) {
        return Proxy.newProxyInstance(Services.class.getClassLoader(), new Class<?>[]{type}, new Inert(what));
    }

    static final class Inert implements InvocationHandler {
        final String what;

        Inert(String what) { this.what = what; }

        @Override
        public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
            switch (m.getName()) {
                case "toString": return what;
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
                case "name": if (m.getReturnType() == String.class) return what;
                default: break;
            }
            if (m.isDefault()) {
                try { return InvocationHandler.invokeDefault(proxy, m, args); } catch (Throwable ignored) {}
            }
            Class<?> r = m.getReturnType();
            if (r == void.class) return null;
            if (r == boolean.class) return false;
            if (r.isPrimitive()) return Conv.zero(r);
            if (r == String.class) return what;
            if (r == Optional.class) return Optional.empty();
            if (List.class.isAssignableFrom(r) || r == Collection.class) return new ArrayList<>();
            if (Set.class.isAssignableFrom(r)) return new HashSet<>();
            if (Map.class.isAssignableFrom(r)) return new HashMap<>();
            if (r.isInterface()) return inert(r, what + "." + m.getName());
            if (r.isInstance(proxy)) return proxy;
            return null;
        }
    }
}
