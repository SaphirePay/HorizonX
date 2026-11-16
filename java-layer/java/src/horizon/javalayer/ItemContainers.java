package horizon.javalayer;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import org.bukkit.Bukkit;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.ItemStack;

import java.lang.reflect.Proxy;

/**
 * BlockStateMeta on an item that holds a block's contents (shulker boxes, chests, barrels...): getBlockState() is
 * a snapshot whose inventory comes from the item's `minecraft:container` component, and setBlockState writes that
 * inventory back to the item, as Paper's CraftMetaBlockState does.
 */
public final class ItemContainers {
    private ItemContainers() {}

    /** A snapshot state: its inventory, and the interfaces of the item's block (ShulkerBox, Chest...). */
    public interface Snapshot {
        Inventory horizonInventory();
    }

    static Object state(H item, Object meta) {
        String type = String.valueOf(Native.tryCall(item, "getType", new Object[0]));
        Object material = Conv.fromNative(Native.tryCall(item, "getType", new Object[0]), org.bukkit.Material.class);
        org.bukkit.Material mat = material instanceof org.bukkit.Material m ? m : org.bukkit.Material.CHEST;
        int size = mat.name().endsWith("SHULKER_BOX") ? 27 : mat == org.bukkit.Material.DISPENSER || mat == org.bukkit.Material.DROPPER ? 9 : mat == org.bukkit.Material.HOPPER ? 5 : 27;
        Inventory inv = Bukkit.createInventory(null, size);
        String json = Native.componentJson(item, "minecraft:container");
        if (json != null) {
            JsonElement e = JsonParser.parseString(json);
            if (e.isJsonArray()) {
                for (JsonElement x : e.getAsJsonArray()) {
                    JsonObject slot = x.getAsJsonObject();
                    int index = slot.get("slot").getAsInt();
                    Object raw = Native.itemFromJson(slot.get("item").toString());
                    if (raw != null && index >= 0 && index < size) inv.setItem(index, (ItemStack) Conv.fromNative(raw, ItemStack.class));
                }
            }
        }
        Class<?> face = blockInterface(mat);
        return Proxy.newProxyInstance(ItemContainers.class.getClassLoader(), new Class<?>[]{face, Snapshot.class}, (proxy, m, args) -> {
            switch (m.getName()) {
                case "horizonInventory":
                case "getInventory":
                case "getSnapshotInventory":
                    return inv;
                case "getType": return mat;
                case "getBaseColor", "getPatterns", "numberOfPatterns":
                    if (mat == org.bukkit.Material.SHIELD) return banner(item, m.getName());
                    break;
                case "getBlockData": return mat.isBlock() ? mat.createBlockData() : null;
                case "isPlaced": return false;
                case "update": return true;
                case "toString": return "org.bukkit.craftbukkit.block.Craft" + face.getSimpleName() + "@" + Integer.toHexString(System.identityHashCode(proxy));
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
                default: break;
            }
            if (m.isDefault()) return java.lang.reflect.InvocationHandler.invokeDefault(proxy, m, args);
            Class<?> r = m.getReturnType();
            if (r == boolean.class) return false;
            if (r.isPrimitive()) return Conv.zero(r);
            return null;
        });
    }

    private static Class<?> blockInterface(org.bukkit.Material mat) {
        String n = mat.name();
        if (n.endsWith("SHULKER_BOX")) return org.bukkit.block.ShulkerBox.class;
        if (n.equals("BARREL")) return org.bukkit.block.Barrel.class;
        if (n.equals("HOPPER")) return org.bukkit.block.Hopper.class;
        if (n.equals("DISPENSER")) return org.bukkit.block.Dispenser.class;
        if (n.equals("DROPPER")) return org.bukkit.block.Dropper.class;
        if (n.endsWith("CHEST")) return org.bukkit.block.Chest.class;
        // A shield's state is a banner (CraftMetaShield); other block entities their block's own interface.
        if (n.equals("SHIELD")) return org.bukkit.block.Banner.class;
        for (Class<?> f : BlockStates.faces(mat)) return f;
        return org.bukkit.block.TileState.class;
    }

    /** A shield's banner: its base colour and patterns (the base_color and banner_patterns components). */
    private static Object banner(H item, String name) {
        if (name.equals("getBaseColor")) {
            String json = Native.componentJson(item, "minecraft:base_color");
            return json == null ? org.bukkit.DyeColor.WHITE : org.bukkit.DyeColor.valueOf(JsonParser.parseString(json).getAsString().toUpperCase(java.util.Locale.ROOT));
        }
        java.util.List<org.bukkit.block.banner.Pattern> out = new java.util.ArrayList<>();
        String json = Native.componentJson(item, "minecraft:banner_patterns");
        if (json != null && JsonParser.parseString(json).isJsonArray()) for (JsonElement e : JsonParser.parseString(json).getAsJsonArray()) {
            JsonObject p = e.getAsJsonObject();
            org.bukkit.block.banner.PatternType type = org.bukkit.Registry.BANNER_PATTERN.get(org.bukkit.NamespacedKey.fromString(p.get("pattern").getAsString()));
            if (type != null) out.add(new org.bukkit.block.banner.Pattern(org.bukkit.DyeColor.valueOf(p.get("color").getAsString().toUpperCase(java.util.Locale.ROOT)), type));
        }
        return name.equals("numberOfPatterns") ? (Object) out.size() : out;
    }

    /** setBlockState(snapshot): the snapshot's inventory back into the item's container component. */
    static void setState(H item, Object state) {
        Inventory inv = state instanceof Snapshot s ? s.horizonInventory() : state instanceof org.bukkit.block.Container c ? c.getInventory() : null;
        if (inv == null) return;
        JsonArray out = new JsonArray();
        ItemStack[] contents = inv.getContents();
        for (int i = 0; i < contents.length; i++) {
            ItemStack stack = contents[i];
            if (stack == null || stack.isEmpty()) continue;
            String json = Native.itemToJson(Conv.toNative(stack));
            if (json == null) continue;
            JsonObject slot = new JsonObject();
            slot.addProperty("slot", i);
            slot.add("item", JsonParser.parseString(json));
            out.add(slot);
        }
        Native.setComponentJson(item, "minecraft:container", out.toString());
    }
}
