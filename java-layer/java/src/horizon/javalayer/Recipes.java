package horizon.javalayer;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import org.bukkit.Material;
import org.bukkit.inventory.*;

import java.util.Locale;
import java.util.Map;

/**
 * `Server.addRecipe` of any Bukkit recipe: written as the game's own recipe JSON (shaped, shapeless, the four
 * cooking kinds, stonecutting, smithing, transmute), ingredient choices as item lists, the result item made by the
 * server from the plugin's ItemStack (with its components). The server puts it in a data pack and reloads.
 */
public final class Recipes {
    private Recipes() {}

    /** The recipes plugins added (Bukkit.addRecipe), as they gave them: listed by recipeIterator/getRecipe as on Paper. */
    private static final java.util.Map<org.bukkit.NamespacedKey, Recipe> ADDED = new java.util.concurrent.ConcurrentHashMap<>();

    static Recipe added(org.bukkit.NamespacedKey key) {
        return ADDED.get(key);
    }

    static boolean forget(org.bukkit.NamespacedKey key) {
        return ADDED.remove(key) != null;
    }

    static void forgetAll() {
        ADDED.clear();
    }

    /** The server's recipes, then the plugins' own. */
    static java.util.Iterator<Recipe> iterator(java.util.Iterator<Recipe> game) {
        java.util.List<Recipe> all = new java.util.ArrayList<>();
        if (game != null) game.forEachRemaining(r -> {
            if (!(r instanceof org.bukkit.Keyed k && ADDED.containsKey(k.getKey()))) all.add(r);
        });
        all.addAll(ADDED.values());
        return all.iterator();
    }

    /** getRecipesFor: the server's, then the plugins' whose result is that item. */
    static java.util.List<Recipe> forResult(java.util.List<Recipe> game, ItemStack result) {
        java.util.List<Recipe> all = new java.util.ArrayList<>(game == null ? java.util.List.of() : game);
        for (Recipe r : ADDED.values()) if (r.getResult() != null && result != null && r.getResult().getType() == result.getType()) all.add(r);
        return all;
    }

    static boolean add(Recipe recipe) {
        if (!(recipe instanceof org.bukkit.Keyed keyed)) return false;
        JsonObject json = json(recipe);
        if (json == null) {
            Report.gap("Server.addRecipe(" + recipe.getClass().getSimpleName() + ")");
            return false;
        }
        org.bukkit.NamespacedKey key = keyed.getKey();
        ItemStack result = recipe.getResult();
        Object nativeResult = result == null ? null : Conv.toNative(result);
        boolean ok = Native.addRecipeJson(key.getNamespace(), key.getKey(), json.toString(), nativeResult);
        if (ok) {
            // Listed as Paper lists it: the server's form of the recipe (CraftShapedRecipe's a..i layout), the plugin's result.
            Recipe view = null;
            try {
                view = GameRecipes.make(key, json, result);
            } catch (RuntimeException ignored) {
            }
            ADDED.put(key, view != null ? view : recipe);
        }
        return ok;
    }

    static JsonObject json(Recipe recipe) {
        JsonObject o = new JsonObject();
        if (recipe instanceof ShapedRecipe s) {
            o.addProperty("type", "minecraft:crafting_shaped");
            group(o, s.getGroup());
            o.addProperty("category", s.getCategory().name().toLowerCase(Locale.ROOT));
            JsonArray pattern = new JsonArray();
            for (String row : s.getShape()) pattern.add(row);
            o.add("pattern", pattern);
            JsonObject key = new JsonObject();
            for (Map.Entry<Character, RecipeChoice> e : s.getChoiceMap().entrySet()) {
                JsonElement i = ingredient(e.getValue());
                if (i != null) key.add(String.valueOf(e.getKey()), i);
            }
            o.add("key", key);
        } else if (recipe instanceof ShapelessRecipe s) {
            o.addProperty("type", "minecraft:crafting_shapeless");
            group(o, s.getGroup());
            o.addProperty("category", s.getCategory().name().toLowerCase(Locale.ROOT));
            JsonArray list = new JsonArray();
            for (RecipeChoice c : s.getChoiceList()) {
                JsonElement i = ingredient(c);
                if (i != null) list.add(i);
            }
            o.add("ingredients", list);
        } else if (recipe instanceof CookingRecipe<?> c) {
            String type = c instanceof BlastingRecipe ? "blasting" : c instanceof SmokingRecipe ? "smoking" : c instanceof CampfireRecipe ? "campfire_cooking" : "smelting";
            o.addProperty("type", "minecraft:" + type);
            group(o, c.getGroup());
            o.addProperty("category", c.getCategory().name().toLowerCase(Locale.ROOT));
            o.add("ingredient", ingredient(c.getInputChoice()));
            o.addProperty("experience", c.getExperience());
            o.addProperty("cookingtime", c.getCookingTime());
        } else if (recipe instanceof StonecuttingRecipe s) {
            o.addProperty("type", "minecraft:stonecutting");
            group(o, s.getGroup());
            o.add("ingredient", ingredient(s.getInputChoice()));
        } else if (recipe instanceof SmithingTransformRecipe s) {
            o.addProperty("type", "minecraft:smithing_transform");
            o.add("template", ingredient(s.getTemplate()));
            o.add("base", ingredient(s.getBase()));
            o.add("addition", ingredient(s.getAddition()));
        } else if (recipe instanceof SmithingTrimRecipe s) {
            o.addProperty("type", "minecraft:smithing_trim");
            o.add("template", ingredient(s.getTemplate()));
            o.add("base", ingredient(s.getBase()));
            o.add("addition", ingredient(s.getAddition()));
            try {
                o.addProperty("pattern", s.getTrimPattern().getKey().toString());
            } catch (RuntimeException ignored) {}
        } else if (recipe instanceof SmithingRecipe s) {
            // The old base class itself (no template): a smithing transform with no template.
            o.addProperty("type", "minecraft:smithing_transform");
            o.add("base", ingredient(s.getBase()));
            o.add("addition", ingredient(s.getAddition()));
        } else if (recipe instanceof TransmuteRecipe t) {
            o.addProperty("type", "minecraft:crafting_transmute");
            group(o, t.getGroup());
            o.add("input", ingredient(t.getInput()));
            o.add("material", ingredient(t.getMaterial()));
        } else {
            return null;
        }
        return o;
    }

    private static void group(JsonObject o, String group) {
        if (group != null && !group.isEmpty()) o.addProperty("group", group);
    }

    /** A choice as the game's ingredient: one item id, or a list of them (exact items count by their type). */
    static JsonElement ingredient(RecipeChoice choice) {
        if (choice == null) return null;
        JsonArray ids = new JsonArray();
        if (choice instanceof RecipeChoice.MaterialChoice m) {
            for (Material mat : m.getChoices()) ids.add(mat.getKey().toString());
        } else if (choice instanceof RecipeChoice.ExactChoice e) {
            for (ItemStack s : e.getChoices()) {
                String id = s.getType().getKey().toString();
                if (!ids.contains(new JsonPrimitive(id))) ids.add(id);
            }
        } else {
            try {
                ItemStack s = choice.getItemStack();
                if (s != null) ids.add(s.getType().getKey().toString());
            } catch (RuntimeException ignored) {}
        }
        if (ids.isEmpty()) return null;
        return ids.size() == 1 ? ids.get(0) : ids;
    }
}
