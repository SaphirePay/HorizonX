package horizon.javalayer;

import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/** Finding the method the SDK asks a Java object for: by name and argument count, or its functional one ("*"). */
final class Calls {
    private Calls() {}

    private static final Map<String, Method> FOUND = new ConcurrentHashMap<>();
    private static final Method NONE;

    static {
        try {
            NONE = Object.class.getMethod("hashCode");
        } catch (NoSuchMethodException e) {
            throw new ExceptionInInitializerError(e);
        }
    }

    static Method find(Object target, String name, int count) {
        Class<?> c = target.getClass();
        String key = c.getName() + "#" + name + "/" + count;
        Method m = FOUND.computeIfAbsent(key, k -> {
            Method found = name.equals("*") ? functional(c, count) : byName(c, name, count);
            if (found == null) return NONE;
            try {
                found.setAccessible(true);
            } catch (RuntimeException ignored) {}
            return found;
        });
        return m == NONE ? null : m;
    }

    private static Method byName(Class<?> c, String name, int count) {
        for (Method m : c.getMethods()) if (m.getName().equals(name) && m.getParameterCount() == count && !m.isBridge()) return m;
        for (Class<?> at = c; at != null; at = at.getSuperclass())
            for (Method m : at.getDeclaredMethods()) if (m.getName().equals(name) && m.getParameterCount() == count && !m.isBridge()) return m;
        return null;
    }

    /** The one abstract method of the functional interface the object implements (Runnable.run, Consumer.accept...). */
    private static Method functional(Class<?> c, int count) {
        Deque<Class<?>> todo = new ArrayDeque<>();
        for (Class<?> at = c; at != null; at = at.getSuperclass()) todo.addAll(Arrays.asList(at.getInterfaces()));
        Set<Class<?>> seen = new HashSet<>();
        while (!todo.isEmpty()) {
            Class<?> face = todo.pop();
            if (!seen.add(face)) continue;
            List<Method> abstracts = new ArrayList<>();
            for (Method m : face.getMethods()) {
                if (!Modifier.isAbstract(m.getModifiers())) continue;
                if (isObjectMethod(m)) continue;
                abstracts.add(m);
            }
            if (abstracts.size() == 1 && abstracts.get(0).getParameterCount() == count) return abstracts.get(0);
            todo.addAll(Arrays.asList(face.getInterfaces()));
        }
        // A plain object: its only public method of that arity declared by its own class.
        Method only = null;
        for (Method m : c.getDeclaredMethods()) {
            if (Modifier.isPublic(m.getModifiers()) && !Modifier.isStatic(m.getModifiers()) && m.getParameterCount() == count) {
                if (only != null) return null;
                only = m;
            }
        }
        return only;
    }

    private static boolean isObjectMethod(Method m) {
        try {
            Object.class.getMethod(m.getName(), m.getParameterTypes());
            return true;
        } catch (NoSuchMethodException e) {
            return false;
        }
    }

    static boolean isA(Class<?> c, String name) {
        Deque<Class<?>> todo = new ArrayDeque<>();
        todo.add(c);
        while (!todo.isEmpty()) {
            Class<?> at = todo.pop();
            if (at.getName().equals(name) || at.getSimpleName().equals(name)) return true;
            if (at.getSuperclass() != null) todo.add(at.getSuperclass());
            todo.addAll(Arrays.asList(at.getInterfaces()));
        }
        return false;
    }
}
