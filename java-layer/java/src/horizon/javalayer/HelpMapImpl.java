package horizon.javalayer;

import org.bukkit.command.Command;
import org.bukkit.craftbukkit.help.SimpleHelpMap;
import org.bukkit.help.GenericCommandHelpTopic;
import org.bukkit.help.HelpTopic;
import org.bukkit.help.HelpTopicFactory;

import java.util.*;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ConcurrentSkipListMap;

/**
 * Bukkit's HelpMap: a topic for each command (as CraftBukkit's default topics), and the ones plugins add. Paper's own
 * class (SimpleHelpMap: CommandAPI casts to it and puts topics into its `helpTopics` by reflection), made without its
 * constructor (that reads help.yml through the server's internals); its two maps are the ones used here.
 */
public final class HelpMapImpl extends SimpleHelpMap {
    static final HelpMapImpl INSTANCE = make();

    private static HelpMapImpl make() {
        HelpMapImpl map = Craft.allocate(HelpMapImpl.class);
        Craft.set(map, SimpleHelpMap.class, "helpTopics", new ConcurrentSkipListMap<String, HelpTopic>(String.CASE_INSENSITIVE_ORDER));
        Craft.set(map, SimpleHelpMap.class, "topicFactoryMap", new ConcurrentHashMap<Class<?>, HelpTopicFactory<Command>>());
        return map;
    }

    /** Never run: make() allocates. */
    private HelpMapImpl() {
        super(null);
    }

    @SuppressWarnings("unchecked")
    private Map<String, HelpTopic> added() {
        return (Map<String, HelpTopic>) Craft.get(this, SimpleHelpMap.class, "helpTopics");
    }

    @SuppressWarnings("unchecked")
    private Map<Class<?>, HelpTopicFactory<?>> factories() {
        return (Map<Class<?>, HelpTopicFactory<?>>) Craft.get(this, SimpleHelpMap.class, "topicFactoryMap");
    }

    @Override
    public HelpTopic getHelpTopic(String name) {
        if (name == null) return null;
        HelpTopic topic = added().get(name);
        if (topic != null) return topic;
        String bare = name.startsWith("/") ? name.substring(1) : name;
        Command command = Commands.MAP.getCommand(bare);
        return command == null ? null : topicOf(command);
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private HelpTopic topicOf(Command command) {
        for (Map.Entry<Class<?>, HelpTopicFactory<?>> f : factories().entrySet()) {
            if (f.getKey().isInstance(command)) {
                HelpTopic t = ((HelpTopicFactory) f.getValue()).createTopic(command);
                if (t != null) return t;
            }
        }
        return new GenericCommandHelpTopic(command);
    }

    @Override
    public Collection<HelpTopic> getHelpTopics() {
        Map<String, HelpTopic> out = new TreeMap<>(String.CASE_INSENSITIVE_ORDER);
        for (Command c : Commands.MAP.getCommands()) out.putIfAbsent("/" + c.getName(), topicOf(c));
        out.putAll(added());
        return out.values();
    }

    /** SimpleHelpMap.addTopic: a name already there keeps its topic. */
    @Override public void addTopic(HelpTopic topic) { added().putIfAbsent(topic.getName(), topic); }
    @Override public void clear() { added().clear(); }
    @Override public void registerHelpTopicFactory(Class commandClass, HelpTopicFactory factory) { factories().put(commandClass, factory); }
    @Override public List<String> getIgnoredPlugins() { return new ArrayList<>(); }
    @Override public void initializeGeneralTopics() {}
    @Override public void initializeCommands() {}
}
