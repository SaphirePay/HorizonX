package horizon.javalayer.nms;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.mojang.serialization.JsonOps;
import org.bukkit.Bukkit;

/**
 * Items between the layer and vanilla's own ItemStack, for Paper's CraftItemStack statics (asNMSCopy, asBukkitCopy:
 * NBT-API, cloud, PacketEvents call them). Through the item's JSON and vanilla's ItemStack.CODEC on the hollow
 * server's registries. The build patches Paper's CraftItemStack to come here for the layer's items (PatchServer).
 */
public final class NmsItems {
    private NmsItems() {}

    /** CraftItemStack.asNMSCopy of a stack that isn't Paper's CraftItemStack (the layer's HItemStack, or Bukkit's). */
    public static net.minecraft.world.item.ItemStack toNms(org.bukkit.inventory.ItemStack stack) {
        if (stack == null || stack.isEmpty()) return net.minecraft.world.item.ItemStack.EMPTY;
        Hollow.boot();
        JsonObject json = Bukkit.getUnsafe().serializeItemAsJson(stack);
        json.remove("DataVersion");
        var ops = Hollow.registryAccess().createSerializationContext(JsonOps.INSTANCE);
        return net.minecraft.world.item.ItemStack.CODEC.parse(ops, json).result().orElse(net.minecraft.world.item.ItemStack.EMPTY);
    }

    /** CraftItemStack.asBukkitMirror: a copy here (the layer's items are the server's, not the vanilla stack). */
    public static org.bukkit.inventory.ItemStack mirror(net.minecraft.world.item.ItemStack nms) {
        return toBukkit(nms);
    }

    /** CraftItemStack.getItemMeta(vanilla stack): the meta of the same item here. */
    public static org.bukkit.inventory.meta.ItemMeta meta(net.minecraft.world.item.ItemStack nms) {
        return meta(nms, null);
    }

    public static org.bukkit.inventory.meta.ItemMeta meta(net.minecraft.world.item.ItemStack nms, org.bukkit.inventory.ItemType type) {
        org.bukkit.inventory.ItemStack b = toBukkit(nms);
        if (type != null && b.getType() != type.asMaterial()) b = b.withType(type.asMaterial());
        return b.getItemMeta();
    }

    public static org.bukkit.inventory.meta.ItemMeta meta(net.minecraft.world.item.ItemStack nms, org.bukkit.inventory.ItemType type, java.util.Set<?> components) {
        return meta(nms, type);
    }

    /** CraftItemStack.setItemMeta(vanilla stack, meta): the meta's components put on the vanilla stack. */
    public static boolean setMeta(net.minecraft.world.item.ItemStack nms, org.bukkit.inventory.meta.ItemMeta meta) {
        if (nms == null || nms.isEmpty()) return false;
        org.bukkit.inventory.ItemStack b = toBukkit(nms);
        boolean ok = b.setItemMeta(meta);
        net.minecraft.world.item.ItemStack made = toNms(b);
        if (!made.isEmpty()) nms.restorePatch(made.getComponentsPatch());
        return ok;
    }

    public static void applyMeta(net.minecraft.world.item.ItemStack nms, org.bukkit.inventory.meta.ItemMeta meta) {
        setMeta(nms, meta);
    }

    /** CraftItemStack.hasItemMeta(vanilla stack): it has components of its own. */
    public static boolean hasMeta(net.minecraft.world.item.ItemStack nms) {
        return nms != null && !nms.isEmpty() && !nms.getComponentsPatch().isEmpty();
    }

    /** CraftItemStack.asBukkitCopy: the layer's own item, made from the vanilla stack's JSON. */
    public static org.bukkit.inventory.ItemStack toBukkit(net.minecraft.world.item.ItemInstance instance) {
        if (!(instance instanceof net.minecraft.world.item.ItemStack nms) || nms.isEmpty()) return org.bukkit.inventory.ItemStack.empty();
        Hollow.boot();
        var ops = Hollow.registryAccess().createSerializationContext(JsonOps.INSTANCE);
        JsonElement json = net.minecraft.world.item.ItemStack.CODEC.encodeStart(ops, nms).result().orElse(null);
        if (json == null || !json.isJsonObject()) return org.bukkit.inventory.ItemStack.empty();
        JsonObject o = json.getAsJsonObject();
        o.addProperty("DataVersion", Bukkit.getUnsafe().getDataVersion());
        return Bukkit.getUnsafe().deserializeItemFromJson(o);
    }
}
