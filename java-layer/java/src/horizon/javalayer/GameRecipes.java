package horizon.javalayer;

import org.bukkit.Bukkit;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Tag;
import org.bukkit.inventory.BlastingRecipe;
import org.bukkit.inventory.CampfireRecipe;
import org.bukkit.inventory.ComplexRecipe;
import org.bukkit.inventory.FurnaceRecipe;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.Recipe;
import org.bukkit.inventory.RecipeChoice;
import org.bukkit.inventory.ShapedRecipe;
import org.bukkit.inventory.ShapelessRecipe;
import org.bukkit.inventory.SmithingTransformRecipe;
import org.bukkit.inventory.SmithingTrimRecipe;
import org.bukkit.inventory.SmokingRecipe;
import org.bukkit.inventory.StonecuttingRecipe;
import org.bukkit.inventory.TransmuteRecipe;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

/**
 * The server's recipes as the Bukkit classes Paper gives (CraftShapedRecipe is a ShapedRecipe...): made from the
 * recipe's data-pack JSON the SDK holds (GameRecipe: key + data), so `recipe instanceof ShapedRecipe`, getShape,
 * getIngredientMap, getChoiceMap, getInputChoice, getCookingTime... answer as on Paper, and every one is Keyed.
 */
public final class GameRecipes {
    private GameRecipes() {}

    /** Made once per key: the game's recipes don't change while it runs (a datapack reload clears it). */
    private static final Map<String, Recipe> MADE = new ConcurrentHashMap<>();

    static void forget() {
        MADE.clear();
    }

    /** The Bukkit recipe for a GameRecipe handle, or null when the handle is not one (or its type is unknown). */
    static Recipe of(H h) {
        Object raw = Native.tryCall(h, "$recipeJson", new Object[0]);
        if (raw == null || raw == Native.MISSING) return null;
        Object made = Conv.fromNative(raw, List.class);
        if (!(made instanceof List<?> pair) || pair.size() < 2) return null;
        String key = String.valueOf(pair.get(0));
        String json = String.valueOf(pair.get(1));
        Recipe cached = MADE.get(key);
        if (cached != null) return cached;
        Recipe r;
        try {
            r = make(NamespacedKey.fromString(key), com.google.gson.JsonParser.parseString(json).getAsJsonObject());
        } catch (RuntimeException e) {
            Logs.warning("recipe " + key + ": " + e);
            return null;
        }
        if (r != null) MADE.put(key, r);
        return r;
    }

    /** What CraftServer gives back for a recipe a plugin added: the server's own form of it, the plugin's result item. */
    static Recipe make(NamespacedKey key, com.google.gson.JsonObject j, ItemStack result) {
        RESULT.set(result);
        try {
            return make(key, j);
        } finally {
            RESULT.remove();
        }
    }

    private static final ThreadLocal<ItemStack> RESULT = new ThreadLocal<>();

    @SuppressWarnings("deprecation")
    static Recipe make(NamespacedKey key, com.google.gson.JsonObject j) {
        String type = str(j, "type").replace("minecraft:", "");
        switch (type) {
            case "crafting_shaped": {
                ShapedRecipe r = new ShapedRecipe(key, result(j.get("result")));
                // CraftShapedRecipe's layout: one letter per cell from 'a' (row by row), an empty cell's letter with no
                // ingredient, each ingredient its own letter even when the data pack shares one.
                List<String> pattern = new ArrayList<>();
                for (var row : j.getAsJsonArray("pattern")) pattern.add(row.getAsString());
                int width = pattern.stream().mapToInt(String::length).max().orElse(0);
                String[] rows = new String[pattern.size()];
                for (int y = 0; y < rows.length; y++) {
                    StringBuilder b = new StringBuilder();
                    for (int x = 0; x < width; x++) b.append((char) ('a' + y * width + x));
                    rows[y] = b.toString();
                }
                r.shape(rows);
                var keys = j.getAsJsonObject("key");
                for (int y = 0; y < rows.length; y++) {
                    String row = pattern.get(y);
                    for (int x = 0; x < row.length(); x++) {
                        String cell = String.valueOf(row.charAt(x));
                        if (row.charAt(x) == ' ' || !keys.has(cell)) continue;
                        RecipeChoice c = choice(keys.get(cell));
                        if (c != null) r.setIngredient((char) ('a' + y * width + x), c);
                    }
                }
                craftingBits(r, j);
                return r;
            }
            case "crafting_shapeless": {
                ShapelessRecipe r = new ShapelessRecipe(key, result(j.get("result")));
                for (var e : j.getAsJsonArray("ingredients")) {
                    RecipeChoice c = choice(e);
                    if (c != null) r.addIngredient(c);
                }
                craftingBits(r, j);
                return r;
            }
            case "smelting":
            case "blasting":
            case "smoking":
            case "campfire_cooking": {
                ItemStack out = result(j.get("result"));
                RecipeChoice in = choice(j.get("ingredient"));
                float xp = j.has("experience") ? j.get("experience").getAsFloat() : 0f;
                int time = j.has("cookingtime") ? j.get("cookingtime").getAsInt() : switch (type) {
                    case "smelting" -> 200;
                    case "campfire_cooking" -> 600;
                    default -> 100;
                };
                org.bukkit.inventory.CookingRecipe<?> r = switch (type) {
                    case "smelting" -> new FurnaceRecipe(key, out, in, xp, time);
                    case "blasting" -> new BlastingRecipe(key, out, in, xp, time);
                    case "smoking" -> new SmokingRecipe(key, out, in, xp, time);
                    default -> new CampfireRecipe(key, out, in, xp, time);
                };
                if (j.has("group")) r.setGroup(str(j, "group"));
                if (j.has("category")) {
                    try {
                        r.setCategory(org.bukkit.inventory.recipe.CookingBookCategory.valueOf(str(j, "category").toUpperCase(Locale.ROOT)));
                    } catch (IllegalArgumentException ignored) {
                    }
                }
                return r;
            }
            case "stonecutting": {
                StonecuttingRecipe r = new StonecuttingRecipe(key, result(j.get("result")), choice(j.get("ingredient")));
                if (j.has("group")) r.setGroup(str(j, "group"));
                return r;
            }
            case "smithing_transform":
                return new SmithingTransformRecipe(key, result(j.get("result")), choice(j.get("template")), choice(j.get("base")), choice(j.get("addition")));
            case "smithing_trim": {
                org.bukkit.inventory.meta.trim.TrimPattern pattern = null;
                if (j.has("pattern")) pattern = Registries.byName(org.bukkit.inventory.meta.trim.TrimPattern.class, str(j, "pattern")) instanceof org.bukkit.inventory.meta.trim.TrimPattern p ? p : null;
                if (pattern != null) return new SmithingTrimRecipe(key, choice(j.get("template")), choice(j.get("base")), choice(j.get("addition")), pattern);
                return new SmithingTrimRecipe(key, choice(j.get("template")), choice(j.get("base")), choice(j.get("addition")));
            }
            case "crafting_transmute": {
                ItemStack out = result(j.get("result"));
                RecipeChoice input = choice(j.get("input"));
                Material made = out.getType();
                // An empty result (map_cloning) keeps the input's item: Bukkit's TransmuteRecipe needs one to name.
                if (made.isAir() && materialChoice(j.get("input")) instanceof RecipeChoice.MaterialChoice mc && !mc.getChoices().isEmpty()) made = mc.getChoices().get(0);
                TransmuteRecipe r = new TransmuteRecipe(key, made, input, choice(j.get("material")));
                craftingBits(r, j);
                return r;
            }
            case "brewing": {
                // data/minecraft/recipe/brewing (26.3): Paper's BrewingRecipe - input and reagent as item choices (the
                // input's potion is not part of the choice there either), the output with its potion.
                var in = j.getAsJsonObject("input");
                var reagent = j.getAsJsonObject("reagent");
                RecipeChoice input = choice(in == null ? null : in.get("item"));
                RecipeChoice ingredient = choice(reagent == null ? null : reagent.get("item"));
                if (input == null || ingredient == null) return null;
                return new org.bukkit.inventory.BrewingRecipe(key, result(j.get("output")), input, ingredient);
            }
            default:
                // crafting_special_*, crafting_decorated_pot...: Paper's CraftComplexRecipe (a ComplexRecipe, Keyed, no result).
                if (type.startsWith("crafting_")) return new Complex(key);
                return null;
        }
    }

    /** CraftComplexRecipe: a recipe the game works out in code (map cloning, firework stars, armor dyes). */
    static final class Complex extends org.bukkit.inventory.CraftingRecipe implements ComplexRecipe {
        Complex(NamespacedKey key) {
            super(key, ItemStack.empty());
        }
    }

    private static void craftingBits(org.bukkit.inventory.CraftingRecipe r, com.google.gson.JsonObject j) {
        if (j.has("group")) r.setGroup(str(j, "group"));
        if (j.has("category")) {
            try {
                r.setCategory(org.bukkit.inventory.recipe.CraftingBookCategory.valueOf(str(j, "category").toUpperCase(Locale.ROOT)));
            } catch (IllegalArgumentException ignored) {
            }
        }
    }

    private static String str(com.google.gson.JsonObject j, String k) {
        return j.has(k) && !j.get(k).isJsonNull() ? j.get(k).getAsString() : "";
    }

    /** An ingredient as Paper 26.3 gives it (CraftRecipe.toBukkit): an ItemTypeRecipeChoice of the item keys, a tag's
     *  items listed out. */
    @SuppressWarnings({"deprecation", "unchecked", "rawtypes"})
    static RecipeChoice choice(com.google.gson.JsonElement e) {
        // A tag ingredient (`#minecraft:planks`) stays the tag, as Paper's NamedRegistryKeySet.
        if (e != null && e.isJsonPrimitive() && e.getAsString().startsWith("#")) {
            try {
                return RecipeChoice.itemType(RegistryTags.get(io.papermc.paper.registry.tag.TagKey.create(io.papermc.paper.registry.RegistryKey.ITEM, net.kyori.adventure.key.Key.key(e.getAsString().substring(1)))));
            } catch (RuntimeException ignored) {
                // An unknown tag: its items as listed below (none: no choice), as before.
            }
        }
        RecipeChoice materials = materialChoice(e);
        if (!(materials instanceof RecipeChoice.MaterialChoice mc)) return materials;
        List<io.papermc.paper.registry.TypedKey<org.bukkit.inventory.ItemType>> keys = new ArrayList<>();
        for (Material m : mc.getChoices()) keys.add(io.papermc.paper.registry.TypedKey.create(io.papermc.paper.registry.RegistryKey.ITEM, m.getKey()));
        try {
            return RecipeChoice.itemType(io.papermc.paper.registry.set.RegistrySet.keySet(io.papermc.paper.registry.RegistryKey.ITEM, (Iterable) keys));
        } catch (RuntimeException ex) {
            return materials;
        }
    }

    /** `"minecraft:stick"`, `"#minecraft:planks"` or a list of them, as materials. */
    @SuppressWarnings("deprecation")
    static RecipeChoice materialChoice(com.google.gson.JsonElement e) {
        if (e == null || e.isJsonNull()) return null;
        if (e.isJsonArray()) {
            List<Material> all = new ArrayList<>();
            for (var x : e.getAsJsonArray()) {
                RecipeChoice c = materialChoice(x);
                if (c instanceof RecipeChoice.MaterialChoice mc) all.addAll(mc.getChoices());
            }
            return all.isEmpty() ? null : new RecipeChoice.MaterialChoice(all);
        }
        if (e.isJsonObject()) {
            // Older form: {"item": ...} / {"tag": ...}.
            var o = e.getAsJsonObject();
            if (o.has("tag")) return materialChoice(new com.google.gson.JsonPrimitive("#" + o.get("tag").getAsString()));
            if (o.has("item")) return materialChoice(o.get("item"));
            return null;
        }
        String s = e.getAsString();
        if (s.startsWith("#")) {
            NamespacedKey tagKey = NamespacedKey.fromString(s.substring(1));
            Tag<Material> tag = tagKey == null ? null : Bukkit.getTag(Tag.REGISTRY_ITEMS, tagKey, Material.class);
            if (tag != null && !tag.getValues().isEmpty()) return new RecipeChoice.MaterialChoice(tag);
            return null;
        }
        Material m = Material.matchMaterial(s);
        return m == null || m.isAir() ? null : new RecipeChoice.MaterialChoice(m);
    }

    /** The result: `{"id", "count", "components"}` (or just an id). */
    static ItemStack result(com.google.gson.JsonElement e) {
        ItemStack given = RESULT.get();
        if (given != null) return given.clone();
        if (e == null || e.isJsonNull()) return ItemStack.empty();
        String id;
        int count = 1;
        com.google.gson.JsonObject components = null;
        if (e.isJsonObject()) {
            var o = e.getAsJsonObject();
            id = o.has("id") ? o.get("id").getAsString() : o.has("item") ? o.get("item").getAsString() : "minecraft:air";
            if (o.has("count")) count = o.get("count").getAsInt();
            if (o.has("components")) components = o.getAsJsonObject("components");
        } else {
            id = e.getAsString();
        }
        Material m = Material.matchMaterial(id);
        if (m == null || m.isAir()) return ItemStack.empty();
        if (components == null || components.isEmpty()) return new ItemStack(m, count);
        // With components (a brewed potion's potion_contents): made by the server from the saved form, as a stack
        // loaded from a config is (component values as SNBT; JSON is SNBT too).
        Map<String, Object> comps = new java.util.LinkedHashMap<>();
        for (var c : components.entrySet()) comps.put(c.getKey(), c.getValue().toString());
        Map<String, Object> saved = new java.util.LinkedHashMap<>();
        saved.put("id", m.getKey().toString());
        saved.put("count", count);
        saved.put("components", comps);
        saved.put("DataVersion", Bukkit.getUnsafe().getDataVersion());
        saved.put("schema_version", 1);
        ItemStack made = Bukkit.getUnsafe().deserializeStack(saved);
        return made == null ? new ItemStack(m, count) : made;
    }
}
