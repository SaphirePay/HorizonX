# HorizonsX

This folder is HorizonsX, the server engine HorizonX runs. It is a fork of
[Horizon](https://github.com/horizon-rust/horizon) (MIT, see [LICENSE](LICENSE)), a Rust rewrite of Minecraft: Java
Edition 26.3. [README.md](README.md) is upstream Horizon's own page.

- **What the fork changes:** [docs/v2/README.md](docs/v2/README.md) lists every change, how it differs from vanilla and
  the switch that turns it off. Folia's model is compared in [docs/v2/folia.md](docs/v2/folia.md).
- **The Paper plugin layer:** [plugin-layer/](plugin-layer). Its `bundle/paper_api` holds the hook points and plugin
  loader; `family/` holds the rows that wire each Bukkit family into the engine. `tools/v2/paper_build.py` applies it
  and builds the engine.
- **Plugins built on the SDK:** [plugins/](plugins).

Snapshot of the `horizonsx` branch at `5c5c267930`: the engine of the HorizonX 0.2.1 release (`7f56ad069b`)
plus items over the wire: an `equippable` and a `use_cooldown` a plugin puts on an item now reach clients (a custom
item's own cooldown group, so its recharge sweep shows on that item alone), with `max_stack_size` and `rarity`.

## Build

```bash
rustup toolchain install 1.99.1
cargo install --git https://github.com/horizon-rust/horizon-mixins
python tools/v2/paper_build.py hostregions
```

The engine lands in `../build/engines/hostregions/horizon-client.exe`.
