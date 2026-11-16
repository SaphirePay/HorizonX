# Programs

Every program HorizonX is made of, compiled for Windows 10 and 11 (64-bit), in one folder and ready to run. It is
the same as the [latest release](https://github.com/SaphirePay/HorizonX/releases/latest) zip.

**Run it:** double-click `HorizonX.exe`. Copy the whole folder anywhere first if you like; the files only need to stay
together. Once it runs, HorizonX keeps itself up to date from the GitHub releases.

| File | What it is | Source |
|---|---|---|
| `HorizonX.exe` | The app: setup wizard, dashboard and server pages. `HorizonX.exe mcp` is its MCP server for AI assistants, `HorizonX.exe update` updates it now, `HorizonX.exe version` prints its version | [`host/`](../host), [`app/`](../app) |
| `engines/horizonsx/horizon-client.exe` | The HorizonsX server engine every server runs | [`horizonsx/`](../horizonsx) |
| `engines/horizonsx/horizon-proxy.exe` | The network proxy: several servers behind one address, `/server` and `/hub` | [`horizonsx/`](../horizonsx) |
| `plugins/*.dll` | The built-in HorizonsX plugins: playit.gg, Network, LuckPerms, CoreProtect, WorldEdit, Multiverse, MiniMOTD, Player Warps, Voice Chat, Elevators | [`horizonsx/plugins/`](../horizonsx/plugins) |
| `plugins/catalog.json` | Their names and descriptions, for the Plugins page | |
| `java/horizon_java.dll` | The Java layer: runs Paper, Spigot and Bukkit plugin jars unchanged | [`java-layer/`](../java-layer) |
| `java/lib/` | The Java layer's jars: the layer itself, the Paper API and its libraries | [`java-layer/`](../java-layer) |
| `java/runtime/` | A Java 25 runtime, Eclipse Temurin (GPLv2 with the Classpath Exception; its notices are inside) | [adoptium.net](https://adoptium.net) |
| `vcruntime140.dll`, `vcruntime140_1.dll` | The Microsoft Visual C++ runtime (redistributable) | Microsoft |

To build them yourself instead, see [Build from source](../README.md#build-from-source).
