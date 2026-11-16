package horizon.javalayer;

import org.bukkit.GameRule;
import org.bukkit.NamespacedKey;

import java.util.function.Function;

/** A game rule under its old type (GameRule's legacy constants), answered by today's rule. */
@SuppressWarnings({"rawtypes", "unchecked"})
final class LegacyRule<L, M> extends GameRule<L> {
    final GameRule<M> modern;
    final Function<L, M> toModern;
    final Function<M, L> fromModern;
    final Class<L> type;

    private LegacyRule(GameRule<M> modern, Function<L, M> toModern, Function<M, L> fromModern, Class<L> type) {
        this.modern = modern;
        this.toModern = toModern;
        this.fromModern = fromModern;
        this.type = type;
    }

    static GameRule of(GameRule modern, Function toModern, Function fromModern, Class type) {
        return new LegacyRule(modern, toModern, fromModern, type);
    }

    @Override public String getName() { return modern.getName(); }
    @Override public Class<L> getType() { return type; }
    @Override public L getDefaultValue() { return fromModern.apply(modern.getDefaultValue()); }
    @Override public String translationKey() { return modern.translationKey(); }
    @Override public NamespacedKey getKey() { return modern.getKey(); }
}
