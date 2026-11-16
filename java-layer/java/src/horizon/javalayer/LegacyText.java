package horizon.javalayer;

import com.google.gson.JsonElement;
import com.google.gson.JsonParser;
import org.bukkit.Material;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.ItemMeta;

/**
 * Legacy text <-> component JSON exactly as Paper's CraftChatMessage does it, by the SDK's port of it (craft_chat.rs),
 * reached through a scratch item's name: what an item's display name would store and read back.
 */
final class LegacyText {
    private LegacyText() {}

    /** `CraftChatMessage.fromString(text)[0]` as JSON. */
    @SuppressWarnings("deprecation")
    static JsonElement toJson(String legacy) {
        if (legacy.isEmpty()) return JsonParser.parseString("\"\"");
        ItemStack scratch = new ItemStack(Material.STONE);
        ItemMeta m = scratch.getItemMeta();
        m.setDisplayName(legacy);
        scratch.setItemMeta(m);
        String json = Native.componentJson(Conv.toNative(scratch), "minecraft:custom_name");
        return json == null ? JsonParser.parseString("\"\"") : JsonParser.parseString(json);
    }

    /** `CraftChatMessage.fromComponent(component)`. */
    @SuppressWarnings("deprecation")
    static String fromJson(JsonElement component) {
        ItemStack scratch = new ItemStack(Material.STONE);
        Object h = Conv.toNative(scratch);
        if (!(h instanceof H)) return component.toString();
        Native.setComponentJson(h, "minecraft:custom_name", component.toString());
        ItemMeta m = scratch.getItemMeta();
        return m == null || !m.hasDisplayName() ? "" : m.getDisplayName();
    }
}
