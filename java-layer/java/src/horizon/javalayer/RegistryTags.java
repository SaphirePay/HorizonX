package horizon.javalayer;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonParser;
import io.papermc.paper.registry.RegistryKey;
import io.papermc.paper.registry.TypedKey;
import io.papermc.paper.registry.tag.Tag;
import io.papermc.paper.registry.tag.TagKey;
import net.kyori.adventure.key.Key;
import org.bukkit.Keyed;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;

import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.NoSuchElementException;
import java.util.Objects;

/**
 * Paper's registry tags (Registry.getTag / hasTag / getTags, io.papermc.paper.registry.tag.Tag): the game's tag of
 * that registry's folder (`item`, `block`, `enchantment`, ...), members read from the server (`tag` / `tags` ops).
 */
final class RegistryTags {
    private RegistryTags() {}

    static String folder(RegistryKey<?> key) {
        return key.key().value();
    }

    /** The member keys of a tag, or null when the registry has no such tag. */
    static List<String> members(RegistryKey<?> registry, Key tag) {
        String answer = Native.query("{\"op\":\"tag\",\"kind\":\"" + folder(registry) + "\",\"name\":\"" + tag.asString() + "\"}");
        if (answer == null) return null;
        JsonElement e = JsonParser.parseString(answer);
        if (!e.isJsonArray()) return null;
        List<String> out = new ArrayList<>();
        for (JsonElement m : e.getAsJsonArray()) out.add(m.getAsString());
        if (out.isEmpty() && !names(registry).contains(tag.asString())) return null;
        return out;
    }

    /** Every tag name of a registry (`minecraft:planks`...). */
    static List<String> names(RegistryKey<?> registry) {
        String answer = Native.query("{\"op\":\"tags\",\"kind\":\"" + folder(registry) + "\"}");
        List<String> out = new ArrayList<>();
        if (answer == null) return out;
        JsonElement e = JsonParser.parseString(answer);
        if (e.isJsonArray()) for (JsonElement m : (JsonArray) e) out.add(m.getAsString().replaceFirst("^#", ""));
        return out;
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    static <T extends Keyed> Tag<T> get(TagKey<T> key) {
        List<String> members = members(key.registryKey(), key.key());
        if (members == null) throw new NoSuchElementException("No tag " + key + " found");
        List<TypedKey<T>> keys = new ArrayList<>();
        for (String m : members) keys.add(TypedKey.create(key.registryKey(), Key.key(m)));
        return new Named<>(key, keys);
    }

    static boolean has(TagKey<?> key) {
        return members(key.registryKey(), key.key()) != null;
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    static Collection<Tag<?>> all(RegistryKey<?> registry) {
        List<Tag<?>> out = new ArrayList<>();
        for (String name : names(registry)) {
            try {
                out.add(get(TagKey.create((RegistryKey) registry, Key.key(name))));
            } catch (RuntimeException ignored) {
            }
        }
        return out;
    }

    /** A named registry tag: its key and its members as typed keys. */
    static final class Named<T extends Keyed> implements Tag<T> {
        private final TagKey<T> key;
        private final List<TypedKey<T>> values;

        Named(TagKey<T> key, List<TypedKey<T>> values) {
            this.key = key;
            this.values = List.copyOf(values);
        }

        @Override public TagKey<T> tagKey() { return key; }
        @Override public RegistryKey<T> registryKey() { return key.registryKey(); }
        @Override public Collection<TypedKey<T>> values() { return values; }
        @Override public boolean contains(TypedKey<T> k) { return values.contains(k); }

        @Override
        public Collection<T> resolve(Registry<T> registry) {
            List<T> out = new ArrayList<>();
            for (TypedKey<T> k : values) {
                T v = registry.get(NamespacedKey.fromString(k.key().asString()));
                if (v != null) out.add(v);
            }
            return out;
        }

        @Override
        public boolean equals(Object o) {
            return o instanceof Named<?> other && other.key.equals(key);
        }

        @Override
        public int hashCode() {
            return Objects.hash(key);
        }

        @Override
        public String toString() {
            // Paper's record (tagKey, namedSet): the game's HolderSet.Named prints its tag and its holders.
            String reg = key.registryKey().key().asString();
            StringBuilder refs = new StringBuilder();
            for (TypedKey<T> k : values) {
                if (refs.length() > 0) refs.append(", ");
                String id = k.key().asString();
                refs.append("Reference{ResourceKey[").append(reg).append(" / ").append(id).append("]=").append(reg.equals("minecraft:block") ? "Block{" + id + "}" : id).append('}');
            }
            return "NamedRegistryKeySetImpl[tagKey=" + key + ", namedSet=NamedSet(TagKey[" + reg + " / " + key.key().asString() + "])[[" + refs + "]]]";
        }
    }
}
