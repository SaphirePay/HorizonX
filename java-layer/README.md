# Java layer

Bukkit, Spigot and Paper plugins (`.jar`) run on the Rust server as they are, with no rewrite.

## How it works

- `rust/` is a normal server plugin (`horizon_java.dll` / `libhorizon_java.so`). When `plugins/` has no `.jar` it does
  nothing: Java is never started and costs no memory.
- When there is a jar, it starts one Java VM (Java 25 or newer) inside the server process, with lean settings: serial
  collector, a heap that gives memory back, a 256 MB cap (`plugins/JavaLayer/config.yml`, `memory:`).
- The plugins see the real Paper 26.3 API. Its interfaces (`Player`, `World`, `Block`, `Inventory`...) are answered by
  the server: each call goes to the SDK by its Bukkit name, through the same binding table jport's converted plugins
  use (`rust/gen.py` writes it for every SDK function). Item stacks are server items (`HItemStack`, as CraftItemStack
  is on Paper). Events: for each event class a Java plugin listens to, the server fires it to Java; a real Bukkit
  event object is filled from it, the listeners run, and what they changed goes back.
- Commands, the scheduler (sync tasks on the server thread each tick, async on a pool), Paper's region schedulers
  (Folia API), services, permissions, configs (`getConfig()` is Bukkit's own YAML code), registries (sounds,
  enchantments, potion effects), tags, and `libraries:` from Maven Central are handled.
- Events: every concrete Paper event the SDK can fire (431 of 443; `rust/api_kinds.txt`), with Bukkit's HandlerList
  rules: a listener of `EntityDamageEvent` also hears `EntityDamageByEntityEvent`, and one hit is delivered once.
- Paper's Brigadier commands (`LifecycleEvents.COMMANDS`, `Commands.register`, `BasicCommand`, `ArgumentTypes.player()`
  and the other vanilla argument types) run on Brigadier in Java.
- Built in: ProtocolLib (its API over the server's packet hooks; a real ProtocolLib jar is skipped, plugins that
  depend on it find it enabled).
- CraftBukkit's names: the server is `org.bukkit.craftbukkit.CraftServer` (with its `commandMap` field), players are
  `CraftPlayer` with CraftHumanEntity's `perm` (LuckPerms puts its Permissible there); `getPluginManager()` is a
  `SimplePluginManager` facade. Rust plugins show up to Java plugins as plugins (in PluginEnableEvent, getPlugin).
- The SDK can call back into Java objects (Runnable, Consumer, listeners, holders).
- Java: the newest installed Java 25+ is used (26 runs plugins built for 26).

## Install on a server

1. Java 25+: anywhere the layer looks (`HORIZON_JAVA_HOME`, `<server>/java-layer/runtime`, `JAVA_HOME`, `java` on the
   PATH, the usual install folders).
2. `<server>/java-layer/lib/`: the contents of `java/out/lib` (`python java/build.py` makes it: the layer's jar, the
   Paper API and the libraries a Paper server gives its plugins).
3. `horizon_java.dll` in `plugins/`, and the plugin jars beside it.

## Build

```
python java/build.py                                   # java/out/lib
python rust/gen.py --check                             # rust/src/generated.rs
cargo +1.99.1 build --release                          # in rust/
```

`runtime/` holds `jrt` and `jbridge`, the Java-runtime crates the bridge is built on. The SDK it calls is `../sdk`.
Horizon Host ships the built layer (`horizon_java.dll`, `java/out/lib` and a Java 25 runtime) and puts it into a server
when a jar is added.

## What doesn't work

- Plugins that call the Java server's own code (`net.minecraft.*`, CraftBukkit internals like `getHandle()`, NMS
  version adapters, a shaded PacketEvents injecting into Netty): there is no Java Minecraft server to call. Netty's
  jars are on the class path, so plugins that only use its buffers load.
- PacketEvents as its own plugin (its API is not built in yet, unlike ProtocolLib's).
- An API call the SDK doesn't have yet throws `UnsupportedOperationException("Horizon's Java layer can't do X yet")`
  and is logged once; the plugin keeps running.
- Plugins compiled for a newer Java than the one installed (class file version errors).

## Coverage

`rust/audit.py` writes `COVERAGE.md`: how many of the SDK's tagged Bukkit methods the layer reaches, and of what Java
hands to the server (server objects' interfaces, events, ItemStack, Bukkit/Server) 98.2%, each gap with its reason.

## Tests

- `test/run.py NAME JAR...`: a scratch server with the jars; prints what the layer said and the engine's memory.
- `test/probe`: LayerProbe, a plugin that checks each API family at start (`python test/probe/build.py`).
- `test/parity.py JAR SCRIPT`: real Paper against this server with the same jar (plugin-tools' parity.py).
