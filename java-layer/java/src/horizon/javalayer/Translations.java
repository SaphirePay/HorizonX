package horizon.javalayer;

import java.io.InputStream;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.function.Consumer;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.TranslatableComponent;
import net.kyori.adventure.text.TranslationArgument;
import net.kyori.adventure.text.flattener.ComponentFlattener;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer;
import net.kyori.adventure.translation.GlobalTranslator;
import net.kyori.adventure.translation.TranslationStore;

/**
 * Paper's PaperAdventure.FLATTENER: translatable text becomes the game's en_us words ("Stone", not
 * "block.minecraft.stone"), with its arguments in place; keys the game hasn't come from plugins' translation stores.
 * The plain and legacy serializers are made with it, as Paper's providers make them.
 */
public final class Translations {
    private Translations() {}

    private static final Pattern LOCALIZATION_PATTERN = Pattern.compile("%(?:(\\d+)\\$)?s");

    /** The game's en_us.json (in the server jar), read once. */
    private static final Map<String, String> EN_US = load();

    private static Map<String, String> load() {
        try (InputStream in = Translations.class.getClassLoader().getResourceAsStream("assets/minecraft/lang/en_us.json")) {
            if (in == null) return Map.of();
            Map<String, String> out = new HashMap<>();
            com.google.gson.JsonObject all = com.google.gson.JsonParser.parseReader(new InputStreamReader(in, StandardCharsets.UTF_8)).getAsJsonObject();
            for (Map.Entry<String, com.google.gson.JsonElement> e : all.entrySet()) out.put(e.getKey(), e.getValue().getAsString());
            return out;
        } catch (Exception e) {
            return Map.of();
        }
    }

    /** The game's English text of a translation key, or null. */
    public static String english(String key) { return EN_US.get(key); }

    public static final ComponentFlattener FLATTENER = ComponentFlattener.basic().toBuilder()
            .complexMapper(TranslatableComponent.class, Translations::flatten)
            .build();

    private static void flatten(TranslatableComponent translatable, Consumer<Component> consumer) {
        String key = translatable.key();
        if (!EN_US.containsKey(key)) {
            for (net.kyori.adventure.translation.Translator source : GlobalTranslator.translator().sources()) {
                if (source instanceof TranslationStore<?> && source.canTranslate(key, Locale.US)) {
                    consumer.accept(GlobalTranslator.render(translatable, Locale.US));
                    return;
                }
            }
        }
        String fallback = translatable.fallback();
        String translated = EN_US.getOrDefault(key, fallback != null ? fallback : key);
        Matcher matcher = LOCALIZATION_PATTERN.matcher(translated);
        List<TranslationArgument> args = translatable.arguments();
        int argPosition = 0;
        int lastIdx = 0;
        while (matcher.find()) {
            if (lastIdx < matcher.start()) consumer.accept(Component.text(translated.substring(lastIdx, matcher.start())));
            lastIdx = matcher.end();
            String argIdx = matcher.group(1);
            if (argIdx != null) {
                try {
                    int idx = Integer.parseInt(argIdx) - 1;
                    if (idx < args.size()) consumer.accept(args.get(idx).asComponent());
                } catch (NumberFormatException ignored) {
                }
            } else {
                int idx = argPosition++;
                if (idx < args.size()) consumer.accept(args.get(idx).asComponent());
            }
        }
        if (lastIdx < translated.length()) consumer.accept(Component.text(translated.substring(lastIdx)));
    }

    /** LegacyComponentSerializer's Provider, as Paper's LegacyComponentSerializerProviderImpl. */
    public static final class Legacy implements LegacyComponentSerializer.Provider {
        public Legacy() {}

        @Override
        public LegacyComponentSerializer legacyAmpersand() {
            return LegacyComponentSerializer.builder().flattener(FLATTENER).character('&').hexColors().useUnusualXRepeatedCharacterHexFormat().build();
        }

        @Override
        public LegacyComponentSerializer legacySection() {
            return LegacyComponentSerializer.builder().flattener(FLATTENER).character(LegacyComponentSerializer.SECTION_CHAR).hexColors().useUnusualXRepeatedCharacterHexFormat().build();
        }

        @Override
        public Consumer<LegacyComponentSerializer.Builder> legacy() {
            return builder -> builder.flattener(FLATTENER);
        }
    }

    /** PlainTextComponentSerializer's Provider, as Paper's PlainTextComponentSerializerProviderImpl. */
    public static final class Plain implements PlainTextComponentSerializer.Provider {
        public Plain() {}

        @Override
        public PlainTextComponentSerializer plainTextSimple() {
            return PlainTextComponentSerializer.builder().flattener(FLATTENER).build();
        }

        @Override
        public Consumer<PlainTextComponentSerializer.Builder> plainText() {
            return builder -> builder.flattener(FLATTENER);
        }
    }
}
