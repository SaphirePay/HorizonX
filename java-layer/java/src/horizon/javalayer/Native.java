package horizon.javalayer;

/** The Rust server. Registered by the layer's DLL when it starts this VM. */
public final class Native {
    private Native() {}

    /** What {@link #tryCall} answers when the SDK has no such method. */
    public static final Object MISSING = new Object() {
        @Override public String toString() { return "MISSING"; }
    };

    /** A method of a game object (an {@link H}) by its Bukkit name. Throws what the SDK threw. */
    public static native Object call(Object self, String name, Object[] args);
    /** As {@link #call}, but {@link #MISSING} when the SDK has no such method. */
    public static native Object tryCall(Object self, String name, Object[] args);
    /** A static (`Bukkit.getOnlinePlayers()`) by its Bukkit class and name. */
    public static native Object callStatic(String cls, String name, Object[] args);
    public static native Object tryStatic(String cls, String name, Object[] args);
    /** `new Location(...)` and the like, for the SDK's own types; {@link #MISSING} when it has none. */
    public static native Object construct(String cls, Object[] args);
    /** Java no longer holds the object with this id. */
    public static native void release(long id);
    /** 0 info, 1 warning, 2 severe. */
    public static native void log(int level, String line);
    public static native boolean registerCommand(String plugin, String name, String[] aliases, String description, String usage, String permission);
    /** The SDK listener for this Bukkit event class at priority slot 0..5; false when the server doesn't fire it. */
    public static native boolean wantEvent(String eventClass, int slot);
    public static native void addPermission(String name, String def, String description);
    public static native boolean isMainThread();
    /** What other threads asked of the server while this one was busy: answered now. */
    public static native void pumpQueries();
    /** Game objects Java holds now (for leak checks). */
    public static native int handles();
    /** Every Bukkit event class the server can fire to Java. */
    public static native String[] eventClasses();
    /** A static field the SDK or a bridge answers (`PacketType.Play.Server.X`); {@link #MISSING} when none. */
    public static native Object field(String cls, String name);
    /** The game's tag `name` in a folder: `isTagged(value)` (a Boolean), or its members (String[]) when value is null. */
    public static native Object tag(String registry, String name, String value);
    /** A query to the engine, JSON in and out (horizon_mod::query). */
    public static native String query(String json);
    /** Server.addRecipe of a recipe already in the game's JSON form; `result` is the result item's handle. */
    /** An item's data component as JSON text, or null. */
    public static native String componentJson(Object item, String id);
    public static native String itemToJson(Object item);
    /** A PlayerProfile's properties as JSON [[name, value, signature], ...]. */
    public static native String profileProperties(Object profile);
    public static native Object itemFromJson(String json);
    /** The JSON of an SDK value object (a built data component...), or null. */
    public static native String valueJson(Object value);
    public static native boolean setComponentJson(Object item, String id, String json);
    public static native boolean addRecipeJson(String namespace, String key, String json, Object result);
}
