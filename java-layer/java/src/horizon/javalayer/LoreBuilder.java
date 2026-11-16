package horizon.javalayer;

import io.papermc.paper.datacomponent.item.ItemLore;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.ComponentLike;

import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.List;

/** `ItemLore.lore()`: a builder of lines; build() is the server's ItemLore of them. */
public final class LoreBuilder {
    private LoreBuilder() {}

    /** `ItemEnchantments.itemEnchantments()`: a builder of levels; build() is the server's ItemEnchantments. */
    static Object enchantments() {
        java.util.Map<org.bukkit.enchantments.Enchantment, Integer> levels = new java.util.LinkedHashMap<>();
        return Proxy.newProxyInstance(LoreBuilder.class.getClassLoader(), new Class<?>[]{io.papermc.paper.datacomponent.item.ItemEnchantments.Builder.class}, (proxy, m, args) -> {
            switch (m.getName()) {
                case "add": levels.put((org.bukkit.enchantments.Enchantment) args[0], (Integer) args[1]); return proxy;
                case "addAll": levels.putAll((java.util.Map<org.bukkit.enchantments.Enchantment, Integer>) args[0]); return proxy;
                case "build": {
                    java.util.Map<String, Integer> byKey = new java.util.LinkedHashMap<>();
                    for (var e : levels.entrySet()) byKey.put(e.getKey().getKey().toString(), e.getValue());
                    Object raw = Native.tryStatic("ItemEnchantments", "itemEnchantments", Conv.args(new Object[]{byKey}));
                    if (raw == Native.MISSING) raw = Native.tryStatic("ItemEnchantments", "itemEnchantments", Conv.args(new Object[]{levels}));
                    if (raw == Native.MISSING) throw Report.cant("ItemEnchantments.itemEnchantments");
                    return Conv.fromNative(raw, io.papermc.paper.datacomponent.item.ItemEnchantments.class);
                }
                case "toString": return "ItemEnchantments.Builder" + levels;
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
                default: break;
            }
            throw Report.cant("ItemEnchantments.Builder." + m.getName());
        });
    }

    @SuppressWarnings("unchecked")
    static Object make() {
        List<Component> lines = new ArrayList<>();
        return Proxy.newProxyInstance(LoreBuilder.class.getClassLoader(), new Class<?>[]{ItemLore.Builder.class}, (proxy, m, args) -> {
            switch (m.getName()) {
                case "lines":
                    lines.clear();
                    for (Object o : (List<?>) args[0]) lines.add(((ComponentLike) o).asComponent());
                    return proxy;
                case "addLine":
                    lines.add(((ComponentLike) args[0]).asComponent());
                    return proxy;
                case "addLines":
                    for (Object o : (List<?>) args[0]) lines.add(((ComponentLike) o).asComponent());
                    return proxy;
                case "build": {
                    Object raw = Native.tryStatic("ItemLore", "lore", Conv.args(new Object[]{new ArrayList<>(lines)}));
                    if (raw == Native.MISSING) throw Report.cant("ItemLore.lore");
                    return Conv.fromNative(raw, ItemLore.class);
                }
                case "toString": return "ItemLore.Builder" + lines;
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
                default: break;
            }
            if (m.isDefault()) return java.lang.reflect.InvocationHandler.invokeDefault(proxy, m, args);
            throw Report.cant("ItemLore.Builder." + m.getName());
        });
    }
}
