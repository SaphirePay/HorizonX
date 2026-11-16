package horizon.javalayer;

import java.lang.reflect.Field;

/**
 * Objects of Paper's own classes made without their constructors (which need a live vanilla server), and their
 * fields set directly: the layer's CraftPlayer is a real CraftEntity (so vanilla code that expects one loads and
 * runs), filled with what its final methods read.
 */
public final class Craft {
    private Craft() {}

    static final sun.misc.Unsafe U;

    static {
        try {
            Field f = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
            f.setAccessible(true);
            U = (sun.misc.Unsafe) f.get(null);
        } catch (ReflectiveOperationException e) {
            throw new ExceptionInInitializerError(e);
        }
    }

    @SuppressWarnings("unchecked")
    public static <T> T allocate(Class<T> type) {
        try {
            return (T) U.allocateInstance(type);
        } catch (InstantiationException e) {
            throw new IllegalStateException("can't make " + type.getName(), e);
        }
    }

    /** Sets a field (final or not) declared by `owner` on `target` (or the static one when target is null). */
    @SuppressWarnings("deprecation")
    public static void set(Object target, Class<?> owner, String name, Object value) {
        try {
            Field f = owner.getDeclaredField(name);
            boolean statik = java.lang.reflect.Modifier.isStatic(f.getModifiers());
            Object base = statik ? U.staticFieldBase(f) : target;
            long offset = statik ? U.staticFieldOffset(f) : U.objectFieldOffset(f);
            Class<?> t = f.getType();
            if (t == int.class) U.putInt(base, offset, (Integer) value);
            else if (t == long.class) U.putLong(base, offset, (Long) value);
            else if (t == boolean.class) U.putBoolean(base, offset, (Boolean) value);
            else if (t == float.class) U.putFloat(base, offset, (Float) value);
            else if (t == double.class) U.putDouble(base, offset, (Double) value);
            else if (t == byte.class) U.putByte(base, offset, (Byte) value);
            else if (t == short.class) U.putShort(base, offset, (Short) value);
            else if (t == char.class) U.putChar(base, offset, (Character) value);
            else {
                if (value != null && !t.isInstance(value)) throw new IllegalArgumentException(owner.getSimpleName() + "." + name + " is " + t.getName() + ", not " + value.getClass().getName());
                U.putObject(base, offset, value);
            }
        } catch (NoSuchFieldException e) {
            throw new IllegalStateException(owner.getName() + " has no field " + name, e);
        }
    }

    public static Object get(Object target, Class<?> owner, String name) {
        try {
            Field f = owner.getDeclaredField(name);
            f.setAccessible(true);
            return f.get(target);
        } catch (ReflectiveOperationException | RuntimeException e) {
            try {
                Field f = owner.getDeclaredField(name);
                boolean statik = java.lang.reflect.Modifier.isStatic(f.getModifiers());
                if (f.getType().isPrimitive()) return null;
                return U.getObject(statik ? U.staticFieldBase(f) : target, statik ? U.staticFieldOffset(f) : U.objectFieldOffset(f));
            } catch (NoSuchFieldException e2) {
                throw new IllegalStateException(owner.getName() + " has no field " + name, e2);
            }
        }
    }

    /** One call on the layer object behind a delegating shadow (HPersistentDataContainer). */
    public static Object delegate(Object inner, java.lang.reflect.Method m, Object[] args) {
        try {
            return m.invoke(inner, args);
        } catch (java.lang.reflect.InvocationTargetException e) {
            Throwable t = e.getCause();
            if (t instanceof RuntimeException r) throw r;
            if (t instanceof Error err) throw err;
            throw new RuntimeException(t);
        } catch (IllegalAccessException e) {
            throw new IllegalStateException(e);
        }
    }

    /** A layer object handed out where Paper's own class declares a narrower type (the player's PDC). */
    public static Object narrow(Class<?> wanted, Object value) {
        if (value == null || wanted.isInstance(value)) return value;
        if (wanted == org.bukkit.craftbukkit.persistence.CraftPersistentDataContainer.class) {
            HPersistentDataContainer pdc = allocate(HPersistentDataContainer.class);
            pdc.inner = value;
            return pdc;
        }
        throw new ClassCastException(value.getClass().getName() + " is not " + wanted.getName());
    }

    /** What Paper's CraftEntity's final methods and schedulers read, on an object made by allocate(). */
    public static void entity(org.bukkit.craftbukkit.entity.CraftEntity self, org.bukkit.entity.EntityType type) {
        Class<?> ce = org.bukkit.craftbukkit.entity.CraftEntity.class;
        set(self, ce, "entityType", type);
        set(self, ce, "server", Bridge.server());
        set(self, ce, "additionalAlliedRules", new java.util.HashMap<>());
        set(self, ce, "taskScheduler", new io.papermc.paper.threadedregions.EntityScheduler(self));
        set(self, ce, "apiScheduler", new io.papermc.paper.threadedregions.scheduler.FoliaEntityScheduler(self));
    }
}
