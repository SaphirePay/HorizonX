package horizon.javalayer;

import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.RegisteredServiceProvider;
import org.bukkit.plugin.ServicePriority;
import org.bukkit.plugin.ServicesManager;

import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/** Bukkit's services between Java plugins (Vault's economy, LuckPerms' API...). */
public final class ServicesImpl implements ServicesManager {
    static final ServicesImpl INSTANCE = new ServicesImpl();

    private final Map<Class<?>, List<RegisteredServiceProvider<?>>> providers = new ConcurrentHashMap<>();

    @Override
    public <T> void register(Class<T> service, T provider, Plugin plugin, ServicePriority priority) {
        List<RegisteredServiceProvider<?>> list = providers.computeIfAbsent(service, k -> new ArrayList<>());
        synchronized (list) {
            list.add(new RegisteredServiceProvider<>(service, provider, priority, plugin));
            list.sort((a, b) -> b.getPriority().compareTo(a.getPriority()));
        }
    }

    @Override
    public void unregisterAll(Plugin plugin) {
        for (List<RegisteredServiceProvider<?>> list : providers.values()) synchronized (list) { list.removeIf(p -> p.getPlugin() == plugin); }
    }

    @Override
    public void unregister(Class<?> service, Object provider) {
        List<RegisteredServiceProvider<?>> list = providers.get(service);
        if (list != null) synchronized (list) { list.removeIf(p -> p.getProvider() == provider); }
    }

    @Override
    public void unregister(Object provider) {
        for (List<RegisteredServiceProvider<?>> list : providers.values()) synchronized (list) { list.removeIf(p -> p.getProvider() == provider); }
    }

    @Override
    public <T> T load(Class<T> service) {
        RegisteredServiceProvider<T> r = getRegistration(service);
        return r == null ? null : r.getProvider();
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T> RegisteredServiceProvider<T> getRegistration(Class<T> service) {
        List<RegisteredServiceProvider<?>> list = providers.get(service);
        if (list == null) return null;
        synchronized (list) { return list.isEmpty() ? null : (RegisteredServiceProvider<T>) list.get(0); }
    }

    @Override
    public List<RegisteredServiceProvider<?>> getRegistrations(Plugin plugin) {
        List<RegisteredServiceProvider<?>> out = new ArrayList<>();
        for (List<RegisteredServiceProvider<?>> list : providers.values()) synchronized (list) { for (RegisteredServiceProvider<?> p : list) if (p.getPlugin() == plugin) out.add(p); }
        return out;
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T> Collection<RegisteredServiceProvider<T>> getRegistrations(Class<T> service) {
        List<RegisteredServiceProvider<?>> list = providers.get(service);
        if (list == null) return new ArrayList<>();
        synchronized (list) {
            List<RegisteredServiceProvider<T>> out = new ArrayList<>();
            for (RegisteredServiceProvider<?> p : list) out.add((RegisteredServiceProvider<T>) p);
            return out;
        }
    }

    @Override
    public Collection<Class<?>> getKnownServices() {
        List<Class<?>> out = new ArrayList<>();
        for (Map.Entry<Class<?>, List<RegisteredServiceProvider<?>>> e : providers.entrySet()) if (!e.getValue().isEmpty()) out.add(e.getKey());
        return out;
    }

    @Override public <T> boolean isProvidedFor(Class<T> service) { return getRegistration(service) != null; }
}
