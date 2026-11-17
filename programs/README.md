# Programs

Every program HorizonX is made of, compiled and ready to run: for Windows 10 and 11 (64-bit) in this folder, and for
Linux (64-bit) in [`linux/`](linux). Each is the same as its zip in the
[latest release](https://github.com/SaphirePay/HorizonX/releases/latest).

**Run it on Windows:** double-click `HorizonX.exe`.

**Run it on Linux:** `./linux/HorizonX` (or double-click it). The first start puts HorizonX in your applications menu.
It needs glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+, Fedora 36+, Mint 21+, Arch, SteamOS 3...), X11 or Wayland.
If you downloaded the repository as a zip and the files lost their executable bit:
`chmod +x linux/HorizonX linux/engines/horizonsx/* linux/java/runtime/bin/*`.

Copy the folder anywhere first if you like; the files only need to stay together. Once it runs, HorizonX keeps itself
up to date from the GitHub releases.

## Windows

| File | What it is | Source |
|---|---|---|
| `HorizonX.exe` | The app: setup wizard, dashboard and server pages. `HorizonX.exe mcp` is its MCP server for AI assistants, `HorizonX.exe update` updates it now, `HorizonX.exe version` prints its version | [`host/`](../host), [`app/`](../app) |
| `engines/horizonsx/horizon-client.exe` | The HorizonsX server engine every server runs | [`horizonsx/`](../horizonsx) |
| `engines/horizonsx/horizon-proxy.exe` | The network proxy: several servers behind one address, `/server` and `/hub` | [`horizonsx/`](../horizonsx) |
| `plugins/*.dll` | The 28 built-in HorizonsX plugins (playit.gg, Network, EssentialsX, LuckPerms, CoreProtect, WorldEdit, Multiverse, mcMMO, Jobs, ChestShop, TAB...) | [`horizonsx/plugins/`](../horizonsx/plugins) |
| `plugins/catalog.json` | Their names and descriptions, for the setup wizard and the Plugins page | |
| `java/horizon_java.dll` | The Java layer: runs Paper, Spigot and Bukkit plugin jars unchanged | [`java-layer/`](../java-layer) |
| `java/lib/` | The Java layer's jars: the layer itself, the Paper API and its libraries | [`java-layer/`](../java-layer) |
| `java/runtime/` | A Java 25 runtime, Eclipse Temurin (GPLv2 with the Classpath Exception; its notices are inside) | [adoptium.net](https://adoptium.net) |
| `vcruntime140.dll`, `vcruntime140_1.dll` | The Microsoft Visual C++ runtime (redistributable) | Microsoft |

## Linux (`linux/`)

| File | What it is | Source |
|---|---|---|
| `HorizonX` | The app. `./HorizonX mcp` (AI assistants, also on a server with no screen), `./HorizonX update`, `./HorizonX version` | [`host/`](../host), [`app/`](../app) |
| `engines/horizonsx/horizon-client` | The HorizonsX server engine | [`horizonsx/`](../horizonsx) |
| `engines/horizonsx/horizon-proxy` | The network proxy | [`horizonsx/`](../horizonsx) |
| `plugins/lib*.so` | The same 28 built-in plugins, built for Linux | [`horizonsx/plugins/`](../horizonsx/plugins) |
| `plugins/catalog.json` | Their names and descriptions | |
| `java/libhorizon_java.so` | The Java layer | [`java-layer/`](../java-layer) |
| `java/lib/` | The Java layer's jars (the same on every system) | [`java-layer/`](../java-layer) |
| `java/runtime/` | Java 25 for Linux, Eclipse Temurin (GPLv2 with the Classpath Exception) | [adoptium.net](https://adoptium.net) |

To build them yourself instead, see [Build from source](../README.md#build-from-source).
