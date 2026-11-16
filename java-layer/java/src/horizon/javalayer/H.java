package horizon.javalayer;

/** A game object the server holds: its id there, the Bukkit class it is, a key naming it, an extra hint. */
public final class H {
    public final long id;
    public final String kind;
    public final String key;
    public final String extra;

    public H(long id, String kind, String key, String extra) {
        this.id = id;
        this.kind = kind == null ? "" : kind;
        this.key = key == null ? "" : key;
        this.extra = extra == null ? "" : extra;
    }

    @Override public String toString() { return "H#" + id + "(" + kind + (key.isEmpty() ? "" : " " + key) + ")"; }
}
