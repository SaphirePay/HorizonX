package horizon.javalayer;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import org.bukkit.Bukkit;
import org.bukkit.Material;
import org.bukkit.configuration.serialization.ConfigurationSerializable;
import org.bukkit.inventory.ItemFlag;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.ItemMeta;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * `==: ItemMeta` in a saved config (CraftMetaItem.SerializableMeta): the item meta format plugins wrote before item
 * components (`v: 3700, type: DIAMOND_SWORD, meta: {meta-type: UNSPECIFIC, display-name: ..., enchants: ...}`), which
 * Paper 26.3 still loads. Made into the item's components here, so the API's own ItemStack.deserialize puts it on the
 * stack (`meta` instanceof ItemMeta -> setItemMeta), as on Paper. Registered under the alias "ItemMeta" (Bridge).
 */
@org.bukkit.configuration.serialization.SerializableAs("ItemMeta")
public final class LegacyMeta implements ConfigurationSerializable {
    private LegacyMeta() {}

    @Override
    public Map<String, Object> serialize() {
        return Map.of();
    }

    /** CraftMetaItem's meta types, by a material whose meta is that type. */
    private static final Map<String, Material> BY_TYPE = new LinkedHashMap<>();

    static {
        BY_TYPE.put("UNSPECIFIC", Material.STONE);
        BY_TYPE.put("BANNER", Material.WHITE_BANNER);
        BY_TYPE.put("BOOK", Material.WRITABLE_BOOK);
        BY_TYPE.put("BOOK_SIGNED", Material.WRITTEN_BOOK);
        BY_TYPE.put("SKULL", Material.PLAYER_HEAD);
        BY_TYPE.put("LEATHER_ARMOR", Material.LEATHER_HORSE_ARMOR);
        BY_TYPE.put("COLORABLE_ARMOR", Material.LEATHER_CHESTPLATE);
        BY_TYPE.put("ARMOR", Material.IRON_CHESTPLATE);
        BY_TYPE.put("MAP", Material.FILLED_MAP);
        BY_TYPE.put("POTION", Material.POTION);
        BY_TYPE.put("SPAWN_EGG", Material.ZOMBIE_SPAWN_EGG);
        BY_TYPE.put("ENCHANTED", Material.ENCHANTED_BOOK);
        BY_TYPE.put("FIREWORK", Material.FIREWORK_ROCKET);
        BY_TYPE.put("FIREWORK_EFFECT", Material.FIREWORK_STAR);
        BY_TYPE.put("TILE_ENTITY", Material.CHEST);
        BY_TYPE.put("KNOWLEDGE_BOOK", Material.KNOWLEDGE_BOOK);
        BY_TYPE.put("TROPICAL_FISH_BUCKET", Material.TROPICAL_FISH_BUCKET);
        BY_TYPE.put("AXOLOTL_BUCKET", Material.AXOLOTL_BUCKET);
        BY_TYPE.put("CROSSBOW", Material.CROSSBOW);
        BY_TYPE.put("SUSPICIOUS_STEW", Material.SUSPICIOUS_STEW);
        BY_TYPE.put("ENTITY_TAG", Material.ARMOR_STAND);
        BY_TYPE.put("COMPASS", Material.COMPASS);
        BY_TYPE.put("BUNDLE", Material.BUNDLE);
        BY_TYPE.put("MUSIC_INSTRUMENT", Material.GOAT_HORN);
        BY_TYPE.put("OMINOUS_BOTTLE", Material.OMINOUS_BOTTLE);
        BY_TYPE.put("SHIELD", Material.SHIELD);
    }

    @SuppressWarnings({"deprecation", "unchecked"})
    public static ItemMeta deserialize(Map<String, Object> map) {
        // A meta this layer saved (its serialize() is the stack's own form: id, components...): back the same way.
        if (!map.containsKey("meta-type") && (map.containsKey("components") || map.containsKey("id"))) {
            Map<String, Object> saved = new LinkedHashMap<>(map);
            saved.putIfAbsent("id", "minecraft:stone");
            saved.putIfAbsent("count", 1);
            saved.putIfAbsent("DataVersion", Bukkit.getUnsafe().getDataVersion());
            saved.putIfAbsent("schema_version", 1);
            ItemStack back = Bukkit.getUnsafe().deserializeStack(saved);
            return back == null ? null : back.getItemMeta();
        }
        String type = String.valueOf(map.getOrDefault("meta-type", "UNSPECIFIC")).toUpperCase(Locale.ROOT);
        Material rep = BY_TYPE.getOrDefault(type, Material.STONE);
        JsonObject comps = new JsonObject();
        String customData = null;

        Object name = map.get("display-name");
        if (name != null) comps.add("minecraft:custom_name", text(String.valueOf(name)));
        Object itemName = map.get("item-name");
        if (itemName != null) comps.add("minecraft:item_name", text(String.valueOf(itemName)));
        if (map.get("lore") instanceof List<?> lore) {
            JsonArray lines = new JsonArray();
            for (Object line : lore) lines.add(text(String.valueOf(line)));
            comps.add("minecraft:lore", lines);
        }
        if (map.get("enchants") instanceof Map<?, ?> e) comps.add("minecraft:enchantments", enchants(e));
        if (map.get("stored-enchants") instanceof Map<?, ?> e) comps.add("minecraft:stored_enchantments", enchants(e));
        if (map.get("repair-cost") instanceof Number n) comps.addProperty("minecraft:repair_cost", n.intValue());
        if (map.get("Damage") instanceof Number n && n.intValue() != 0) comps.addProperty("minecraft:damage", n.intValue());
        if (Boolean.TRUE.equals(map.get("Unbreakable"))) comps.add("minecraft:unbreakable", new JsonObject());
        if (map.get("custom-model-data") instanceof Number n) {
            JsonObject cmd = new JsonObject();
            JsonArray floats = new JsonArray();
            floats.add(n.floatValue());
            cmd.add("floats", floats);
            comps.add("minecraft:custom_model_data", cmd);
        }
        if (map.get("max-stack-size") instanceof Number n) comps.addProperty("minecraft:max_stack_size", n.intValue());
        if (map.get("rarity") != null) comps.addProperty("minecraft:rarity", String.valueOf(map.get("rarity")).toLowerCase(Locale.ROOT));
        if (map.get("enchantment-glint-override") instanceof Boolean b) comps.addProperty("minecraft:enchantment_glint_override", b);
        if (map.get("map-id") instanceof Number n) comps.addProperty("minecraft:map_id", n.intValue());
        Object potion = map.get("potion-type");
        if (potion != null || map.get("custom-color") != null) {
            JsonObject pc = new JsonObject();
            if (potion != null) {
                String p = String.valueOf(potion).toLowerCase(Locale.ROOT);
                pc.addProperty("potion", p.contains(":") ? p : "minecraft:" + p);
            }
            if (map.get("custom-color") instanceof org.bukkit.Color c) pc.addProperty("custom_color", c.asARGB());
            comps.add("minecraft:potion_contents", pc);
        }
        // The plugins' own data (PersistentDataContainer): Paper writes the whole compound as one SNBT string; older
        // servers a map of key -> each value's SNBT.
        if (map.get("PublicBukkitValues") instanceof String snbt && !snbt.isBlank()) {
            customData = "{PublicBukkitValues:" + snbt.trim() + "}";
        } else if (map.get("PublicBukkitValues") instanceof Map<?, ?> pdc && !pdc.isEmpty()) {
            StringBuilder b = new StringBuilder("{PublicBukkitValues:{");
            boolean first = true;
            for (Map.Entry<?, ?> e : pdc.entrySet()) {
                if (!first) b.append(',');
                first = false;
                b.append('"').append(String.valueOf(e.getKey()).replace("\\", "\\\\").replace("\"", "\\\"")).append("\":").append(e.getValue());
            }
            customData = b.append("}}").toString();
        }

        ItemStack stack;
        if (customData != null) {
            Map<String, Object> saved = new LinkedHashMap<>();
            saved.put("id", rep.getKey().toString());
            saved.put("count", 1);
            saved.put("components", Map.of("minecraft:custom_data", customData));
            saved.put("DataVersion", Bukkit.getUnsafe().getDataVersion());
            saved.put("schema_version", 1);
            stack = Bukkit.getUnsafe().deserializeStack(saved);
        } else {
            stack = new ItemStack(rep);
        }
        Object h = Conv.toNative(stack);
        if (h instanceof H) {
            for (var e : comps.entrySet()) Native.setComponentJson(h, e.getKey(), e.getValue().toString());
        }
        ItemMeta meta = stack.getItemMeta();
        if (meta == null) return null;
        if (map.get("ItemFlags") instanceof List<?> flags) {
            for (Object f : flags) {
                try {
                    meta.addItemFlags(ItemFlag.valueOf(String.valueOf(f)));
                } catch (IllegalArgumentException ignored) {
                }
            }
        }
        // The typed parts the API sets itself (their objects come in already made by the config: Color, PotionEffect...).
        try {
            if (meta instanceof org.bukkit.inventory.meta.LeatherArmorMeta la && map.get("color") instanceof org.bukkit.Color c) la.setColor(c);
            if (meta instanceof org.bukkit.inventory.meta.PotionMeta pm && map.get("custom-effects") instanceof List<?> effects) {
                for (Object e : effects) if (e instanceof org.bukkit.potion.PotionEffect pe) pm.addCustomEffect(pe, true);
            }
            if (meta instanceof org.bukkit.inventory.meta.SkullMeta sm) {
                Object owner = map.get("skull-owner");
                if (owner instanceof String s && !s.isEmpty()) sm.setOwner(s);
                else if (owner instanceof org.bukkit.profile.PlayerProfile pp) sm.setOwnerProfile(pp);
            }
            if (meta instanceof org.bukkit.inventory.meta.BookMeta bm) {
                if (map.get("title") != null) bm.setTitle(String.valueOf(map.get("title")));
                if (map.get("author") != null) bm.setAuthor(String.valueOf(map.get("author")));
                if (map.get("pages") instanceof List<?> pages) {
                    for (Object page : pages) {
                        String p = String.valueOf(page);
                        var parsed = text(p);
                        bm.addPage(parsed.isJsonPrimitive() ? parsed.getAsString() : LegacyText.fromJson(parsed));
                    }
                }
                if (map.get("generation") instanceof Number g) {
                    org.bukkit.inventory.meta.BookMeta.Generation[] all = org.bukkit.inventory.meta.BookMeta.Generation.values();
                    if (g.intValue() >= 0 && g.intValue() < all.length) bm.setGeneration(all[g.intValue()]);
                }
            }
            if (meta instanceof org.bukkit.inventory.meta.FireworkMeta fm) {
                if (map.get("power") instanceof Number p) fm.setPower(p.intValue());
                if (map.get("firework-effects") instanceof List<?> effects) {
                    for (Object e : effects) if (e instanceof org.bukkit.FireworkEffect fe) fm.addEffect(fe);
                }
            }
            if (meta instanceof org.bukkit.inventory.meta.FireworkEffectMeta fem && map.get("firework-effect") instanceof org.bukkit.FireworkEffect fe) fem.setEffect(fe);
            if (meta instanceof org.bukkit.inventory.meta.BannerMeta bnm && map.get("patterns") instanceof List<?> patterns) {
                for (Object p : patterns) if (p instanceof org.bukkit.block.banner.Pattern pt) bnm.addPattern(pt);
            }
        } catch (RuntimeException e) {
            Logs.warning("ItemMeta (" + type + ") from a saved config: " + e);
        }
        return meta;
    }

    /** A saved name or lore line: JSON (what CraftMetaItem wrote since 1.13), or legacy text (older). */
    static com.google.gson.JsonElement text(String s) {
        String t = s.trim();
        if (t.startsWith("{") || t.startsWith("[") || t.startsWith("\"")) {
            try {
                return JsonParser.parseString(t);
            } catch (RuntimeException ignored) {
            }
        }
        return LegacyText.toJson(s);
    }

    /** `DAMAGE_ALL: 3` (Bukkit's old names) or `minecraft:sharpness: 3`, as the enchantments component. */
    @SuppressWarnings("deprecation")
    static JsonObject enchants(Map<?, ?> in) {
        JsonObject out = new JsonObject();
        for (Map.Entry<?, ?> e : in.entrySet()) {
            String name = String.valueOf(e.getKey());
            String renamed = Bukkit.getUnsafe().get(org.bukkit.enchantments.Enchantment.class, name);
            String key = renamed == null ? name : renamed;
            key = key.toLowerCase(Locale.ROOT);
            if (!key.contains(":")) key = "minecraft:" + key;
            if (e.getValue() instanceof Number n) out.addProperty(key, n.intValue());
        }
        return out;
    }
}
