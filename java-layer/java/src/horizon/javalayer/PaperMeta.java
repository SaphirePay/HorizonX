package horizon.javalayer;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.google.gson.JsonPrimitive;
import org.bukkit.inventory.meta.ItemMeta;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.regex.Pattern;

/**
 * Paper 26.3's text forms of items: CraftMetaItem.toString (`UNSPECIFIC_META:{meta-type=UNSPECIFIC, display-name=...}`,
 * its SerializableMeta map in CraftMetaItem.serializeInto's order), text components as the game's codec writes them
 * (contents, then `extra`, then the style keys), and SNBT as the game prints a tag (sorted keys, bare where it can,
 * 0b/1b). The forms were read off a real Paper 26.3 run (test/jprobe.py --paper).
 */
public final class PaperMeta {
    private PaperMeta() {}

    static final Gson GSON = new GsonBuilder().disableHtmlEscaping().create();

    /** CraftMetaItem's SerializableMeta type of each Paper meta class. */
    static final Map<String, String> TYPES = Map.ofEntries(
            Map.entry("CraftMetaItem", "UNSPECIFIC"), Map.entry("CraftMetaBook", "BOOK"), Map.entry("CraftMetaBookSigned", "BOOK_SIGNED"),
            Map.entry("CraftMetaSkull", "SKULL"), Map.entry("CraftMetaLeatherArmor", "LEATHER_ARMOR"), Map.entry("CraftMetaColorableArmor", "COLORABLE_ARMOR"),
            Map.entry("CraftMetaArmor", "ARMOR"), Map.entry("CraftMetaMap", "MAP"), Map.entry("CraftMetaPotion", "POTION"),
            Map.entry("CraftMetaSpawnEgg", "SPAWN_EGG"), Map.entry("CraftMetaEnchantedBook", "ENCHANTED"), Map.entry("CraftMetaFirework", "FIREWORK"),
            Map.entry("CraftMetaCharge", "FIREWORK_EFFECT"), Map.entry("CraftMetaBlockState", "TILE_ENTITY"), Map.entry("CraftMetaKnowledgeBook", "KNOWLEDGE_BOOK"),
            Map.entry("CraftMetaBanner", "BANNER"), Map.entry("CraftMetaTropicalFishBucket", "TROPICAL_FISH_BUCKET"), Map.entry("CraftMetaAxolotlBucket", "AXOLOTL_BUCKET"),
            Map.entry("CraftMetaCrossbow", "CROSSBOW"), Map.entry("CraftMetaSuspiciousStew", "SUSPICIOUS_STEW"), Map.entry("CraftMetaEntityTag", "ENTITY_TAG"),
            Map.entry("CraftMetaCompass", "COMPASS"), Map.entry("CraftMetaBundle", "BUNDLE"), Map.entry("CraftMetaMusicInstrument", "MUSIC_INSTRUMENT"),
            Map.entry("CraftMetaOminousBottle", "OMINOUS_BOTTLE"), Map.entry("CraftMetaShield", "SHIELD"), Map.entry("CraftMetaArmorStand", "ARMOR_STAND"));

    static String type(ItemMeta meta) {
        return TYPES.getOrDefault(meta.getClass().getSimpleName(), "UNSPECIFIC");
    }

    /** CraftMetaItem.toString. */
    public static String string(ItemMeta meta) {
        String type = type(meta);
        return type + "_META:" + map(meta, type);
    }

    /** One text component of the item (`minecraft:custom_name`), as Paper's CraftChatMessage.toJSON writes it. */
    static String text(H h, String component) {
        String raw = Native.componentJson(h, component);
        return raw == null ? null : GSON.toJson(component(JsonParser.parseString(raw)));
    }

    @SuppressWarnings("deprecation")
    static Map<String, Object> map(ItemMeta meta, String type) {
        Map<String, Object> out = new LinkedHashMap<>();
        out.put("meta-type", type);
        H h = Proxies.handleOf(meta);
        if (h != null && meta.hasDisplayName()) put(out, "display-name", text(h, "minecraft:custom_name"));
        if (h != null && meta.hasItemName()) put(out, "item-name", text(h, "minecraft:item_name"));
        if (h != null && meta.hasLore()) {
            String raw = Native.componentJson(h, "minecraft:lore");
            if (raw != null) {
                List<String> lore = new ArrayList<>();
                for (JsonElement line : JsonParser.parseString(raw).getAsJsonArray()) lore.add(GSON.toJson(component(line)));
                out.put("lore", lore);
            }
        }
        if (meta.hasCustomModelDataComponent()) {
            var c = meta.getCustomModelDataComponent();
            List<Integer> colors = new ArrayList<>();
            for (org.bukkit.Color color : c.getColors()) colors.add(color.asRGB());
            out.put("custom-model-data", "CraftCustomModelDataComponent{component=CustomModelData[floats=" + c.getFloats() + ", flags=" + c.getFlags() + ", strings=" + c.getStrings() + ", colors=" + colors + "]}");
        }
        if (meta.hasEnchants()) out.put("enchants", enchants(meta.getEnchants()));
        if (meta.hasAttributeModifiers()) {
            Map<String, Object> mods = new LinkedHashMap<>();
            for (var e : meta.getAttributeModifiers().asMap().entrySet()) mods.put(e.getKey().getKey().toString(), new ArrayList<>(e.getValue()));
            out.put("attribute-modifiers", mods);
        }
        if (meta instanceof org.bukkit.inventory.meta.Repairable r && r.hasRepairCost()) out.put("repair-cost", r.getRepairCost());
        if (!meta.getItemFlags().isEmpty()) out.put("ItemFlags", new java.util.TreeSet<>(meta.getItemFlags().stream().map(Enum::name).toList()));
        if (meta.isUnbreakable()) out.put("Unbreakable", true);
        if (meta.hasEnchantmentGlintOverride()) out.put("enchantment-glint-override", meta.getEnchantmentGlintOverride());
        if (meta.hasMaxStackSize()) out.put("max-stack-size", meta.getMaxStackSize());
        if (meta.hasRarity()) out.put("rarity", meta.getRarity().name());
        if (meta instanceof org.bukkit.inventory.meta.Damageable d) {
            if (d.hasDamage()) out.put("Damage", d.getDamage());
            if (d.hasMaxDamage()) out.put("max-damage", d.getMaxDamage());
        }
        var pdc = meta.getPersistentDataContainer();
        if (!pdc.isEmpty() && h != null) {
            String raw = Native.componentJson(h, "minecraft:custom_data");
            JsonElement values = raw == null ? null : JsonParser.parseString(raw).getAsJsonObject().get("PublicBukkitValues");
            if (values != null && values.isJsonObject()) {
                Map<String, Object> pub = new TreeMap<>();
                for (var e : values.getAsJsonObject().entrySet()) pub.put(e.getKey(), snbt(e.getValue(), e.getKey()));
                out.put("PublicBukkitValues", pub);
            }
        }
        own(meta, type, h, out);
        return out;
    }

    /** The subclass's own keys, after CraftMetaItem's (each serializeInto calls super first). */
    @SuppressWarnings("deprecation")
    static void own(ItemMeta meta, String type, H h, Map<String, Object> out) {
        switch (type) {
            case "SKULL" -> {
                var s = (org.bukkit.inventory.meta.SkullMeta) meta;
                if (s.hasOwner() && s.getPlayerProfile() != null) out.put("skull-owner", s.getPlayerProfile());
            }
            case "POTION" -> {
                var p = (org.bukkit.inventory.meta.PotionMeta) meta;
                if (p.getBasePotionType() != null) out.put("potion-type", p.getBasePotionType().getKey().toString());
                if (p.hasColor()) out.put("custom-color", p.getColor());
                if (p.hasCustomEffects()) out.put("custom-effects", p.getCustomEffects());
            }
            case "BOOK", "BOOK_SIGNED" -> {
                var b = (org.bukkit.inventory.meta.BookMeta) meta;
                if (b.hasTitle()) out.put("title", b.getTitle());
                if (b.hasAuthor()) out.put("author", b.getAuthor());
                if (b.hasPages()) {
                    if (type.equals("BOOK_SIGNED") && h != null) {
                        String raw = Native.componentJson(h, "minecraft:written_book_content");
                        JsonElement pages = raw == null ? null : JsonParser.parseString(raw).getAsJsonObject().get("pages");
                        if (pages != null && pages.isJsonArray()) {
                            List<String> list = new ArrayList<>();
                            for (JsonElement page : pages.getAsJsonArray()) {
                                JsonElement text = page.isJsonObject() && page.getAsJsonObject().has("raw") ? page.getAsJsonObject().get("raw") : page;
                                list.add(GSON.toJson(component(text)));
                            }
                            out.put("pages", list);
                        }
                    } else {
                        out.put("pages", b.getPages());
                    }
                }
            }
            case "ENCHANTED" -> {
                var e = (org.bukkit.inventory.meta.EnchantmentStorageMeta) meta;
                if (e.hasStoredEnchants()) out.put("stored-enchants", enchants(e.getStoredEnchants()));
            }
            case "LEATHER_ARMOR", "COLORABLE_ARMOR" -> {
                var l = (org.bukkit.inventory.meta.LeatherArmorMeta) meta;
                if (l.isDyed()) out.put("color", l.getColor());
            }
            case "FIREWORK" -> {
                var f = (org.bukkit.inventory.meta.FireworkMeta) meta;
                if (f.hasEffects()) out.put("firework-effects", f.getEffects());
                if (f.hasPower()) out.put("power", f.getPower());
            }
            case "FIREWORK_EFFECT" -> {
                var f = (org.bukkit.inventory.meta.FireworkEffectMeta) meta;
                if (f.hasEffect()) out.put("firework-effect", f.getEffect());
            }
            case "BANNER" -> {
                var b = (org.bukkit.inventory.meta.BannerMeta) meta;
                if (b.numberOfPatterns() > 0) out.put("patterns", b.getPatterns());
            }
            case "TILE_ENTITY" -> {
                ItemStackHolder.material(meta, out);
            }
            default -> {
            }
        }
    }

    /** CraftMetaBlockState keeps the item's material (`blockMaterial=CHEST`). */
    static final class ItemStackHolder {
        static void material(ItemMeta meta, Map<String, Object> out) {
            H h = Proxies.handleOf(meta);
            String json = h == null ? null : Native.itemToJson(h);
            if (json == null) return;
            JsonElement id = JsonParser.parseString(json).getAsJsonObject().get("id");
            if (id == null) return;
            org.bukkit.Material mat = org.bukkit.Material.matchMaterial(id.getAsString());
            if (mat != null) out.put("blockMaterial", mat.name());
        }
    }

    /** PaperItemEnchantments.toString: the game's ItemEnchantments (holder => level) and the API's map. */
    static String enchantmentsString(JsonObject json) {
        StringBuilder game = new StringBuilder(), api = new StringBuilder();
        for (var e : json.entrySet()) {
            org.bukkit.NamespacedKey key = org.bukkit.NamespacedKey.fromString(e.getKey());
            if (key == null) continue;
            String english = Translations.english("enchantment." + key.getNamespace() + "." + key.getKey());
            if (game.length() > 0) {
                game.append(", ");
                api.append(", ");
            }
            game.append("Reference{ResourceKey[minecraft:enchantment / ").append(key).append("]=Enchantment ").append(english != null ? english : key.getKey()).append("}=>").append(e.getValue());
            Object enchant = org.bukkit.Registry.ENCHANTMENT.get(key);
            api.append(enchant).append('=').append(e.getValue());
        }
        return "PaperItemEnchantments[impl=ItemEnchantments{enchantments={" + game + "}}, enchantments={" + api + "}]";
    }

    static Map<String, Integer> enchants(Map<org.bukkit.enchantments.Enchantment, Integer> all) {
        Map<String, Integer> out = new LinkedHashMap<>();
        for (var e : all.entrySet()) out.put(e.getKey().getKey().toString(), e.getValue());
        return out;
    }

    static void put(Map<String, Object> out, String key, Object value) {
        if (value != null) out.put(key, value);
    }

    // ---- text components in the game codec's key order ----

    static final List<String> CONTENTS = List.of("type", "text", "translate", "fallback", "with", "score", "selector", "keybind", "nbt", "interpret",
            "block", "entity", "storage", "source", "separator", "object", "atlas", "sprite", "player", "hat");
    static final List<String> STYLE = List.of("color", "shadow_color", "bold", "italic", "underlined", "strikethrough", "obfuscated", "click_event",
            "hover_event", "insertion", "font");

    /** A text component with its keys in the game codec's order: contents, `extra`, style; children the same. */
    public static JsonElement component(JsonElement e) {
        if (e == null || !e.isJsonObject()) return e;
        JsonObject in = e.getAsJsonObject();
        JsonObject out = new JsonObject();
        for (String k : CONTENTS) if (in.has(k)) out.add(k, k.equals("with") || k.equals("separator") ? children(in.get(k)) : in.get(k));
        if (in.has("extra")) out.add("extra", children(in.get("extra")));
        for (String k : STYLE) if (in.has(k)) out.add(k, in.get(k));
        for (var entry : in.entrySet()) if (!out.has(entry.getKey())) out.add(entry.getKey(), entry.getValue());
        return out;
    }

    static JsonElement children(JsonElement e) {
        if (e == null) return null;
        if (e.isJsonArray()) {
            JsonArray out = new JsonArray();
            for (JsonElement c : e.getAsJsonArray()) out.add(component(c));
            return out;
        }
        return component(e);
    }

    /** The components whose values are text components (reordered in serializeItemAsJson). */
    static final java.util.Set<String> TEXT_COMPONENTS = java.util.Set.of("minecraft:custom_name", "minecraft:item_name", "minecraft:lore");

    public static JsonElement componentValue(String id, JsonElement value) {
        return TEXT_COMPONENTS.contains(id) ? children(value) : value;
    }

    // ---- SNBT as the game prints a tag (StringTagVisitor) ----

    static final Pattern BARE = Pattern.compile("[A-Za-z0-9._+-]+");

    /** A component's JSON as SNBT: keys sorted, bare where they can be, booleans as bytes; `floats` lists as floats. */
    public static String snbt(JsonElement e, String key) {
        StringBuilder s = new StringBuilder();
        write(s, e, key);
        return s.toString();
    }

    static void write(StringBuilder s, JsonElement e, String key) {
        if (e == null || e.isJsonNull()) {
            s.append("{}");
        } else if (e.isJsonObject()) {
            s.append('{');
            boolean first = true;
            for (var entry : new TreeMap<>(e.getAsJsonObject().asMap()).entrySet()) {
                if (!first) s.append(',');
                first = false;
                s.append(BARE.matcher(entry.getKey()).matches() ? entry.getKey() : quote(entry.getKey())).append(':');
                write(s, entry.getValue(), entry.getKey());
            }
            s.append('}');
        } else if (e.isJsonArray()) {
            s.append('[');
            boolean first = true;
            for (JsonElement c : e.getAsJsonArray()) {
                if (!first) s.append(',');
                first = false;
                write(s, c, key);
            }
            s.append(']');
        } else {
            JsonPrimitive p = e.getAsJsonPrimitive();
            if (p.isBoolean()) {
                s.append(p.getAsBoolean() ? "1b" : "0b");
            } else if (p.isNumber()) {
                double d = p.getAsDouble();
                boolean whole = d == Math.rint(d) && !p.getAsString().contains(".") && !p.getAsString().contains("e") && !p.getAsString().contains("E");
                if ("floats".equals(key)) s.append((float) d).append('f');
                else if (whole) s.append(p.getAsString());
                else s.append(d).append('d');
            } else {
                s.append(quote(p.getAsString()));
            }
        }
    }

    /** StringTag.quoteAndEscape: double quotes unless the text has some and no single ones. */
    static String quote(String text) {
        char q = text.indexOf('"') >= 0 && text.indexOf('\'') < 0 ? '\'' : '"';
        StringBuilder s = new StringBuilder().append(q);
        for (char c : text.toCharArray()) {
            if (c == '\\' || c == q) s.append('\\');
            s.append(c);
        }
        return s.append(q).toString();
    }
}
