package horizon.javalayer;

/** A small value or a collection on its way between Java and the server (see the Rust side's conv.rs). */
public final class Pack {
    public static final int LOC = 1, VEC = 2, KEY = 3, JSON = 4, MAT = 5, LIST = 6, SET = 7, ARR = 8, MAP = 9, REF = 10;

    public final int tag;
    public final Object o;
    public final double x, y, z;
    public final float f1, f2;
    public final String s;
    public final Object[] items;
    public final Object[] vals;
    public final long id;

    public Pack(int tag, Object o, double x, double y, double z, float f1, float f2, String s, Object[] items, Object[] vals, long id) {
        this.tag = tag;
        this.o = o;
        this.x = x;
        this.y = y;
        this.z = z;
        this.f1 = f1;
        this.f2 = f2;
        this.s = s;
        this.items = items;
        this.vals = vals;
        this.id = id;
    }

    static Pack of(int tag, String s) { return new Pack(tag, null, 0, 0, 0, 0, 0, s, null, null, 0); }
    static Pack items(int tag, Object[] items) { return new Pack(tag, null, 0, 0, 0, 0, 0, null, items, null, 0); }
}
