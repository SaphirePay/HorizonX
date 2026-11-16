package horizon.javalayer;

import org.bukkit.Material;

/**
 * What Paper 26.3's own classes print for toString(), for the game objects the layer answers with proxies: the
 * CraftBukkit form where it is stable (CraftWorld{key=...}, CraftBlockData{...}), else the CraftBukkit class name
 * and an identity hash, as Object.toString gives on Paper (CraftScoreboard@1a2b3c).
 */
final class PaperStrings {
    private PaperStrings() {}

    static String of(Object proxy) {
        try {
            if (proxy instanceof org.bukkit.inventory.meta.ItemMeta meta) return PaperMeta.string(meta);
            if (proxy instanceof io.papermc.paper.registry.data.dialog.input.TextDialogInput t) {
                var ml = t.multiline();
                return "TextDialogInputImpl[key=" + t.key() + ", width=" + t.width() + ", label=" + t.label() + ", labelVisible=" + t.labelVisible()
                        + ", initial=" + t.initial() + ", maxLength=" + t.maxLength() + ", multiline="
                        + (ml == null ? "null" : "MultilineOptionsImpl[maxLines=" + ml.maxLines() + ", height=" + ml.height() + "]") + "]";
            }
            if (proxy instanceof io.papermc.paper.registry.data.dialog.action.DialogAction.CustomClickAction c) {
                return "CustomClickActionImpl[id=" + c.id().asString() + ", additions=" + c.additions() + "]";
            }
            // CraftEntity.toString: CraftCow{uuid=...} (players print their name, from the server).
            if (proxy instanceof org.bukkit.entity.Entity e && !(proxy instanceof org.bukkit.entity.Player)) {
                Class<?> face = e.getType().getEntityClass();
                return "Craft" + (face != null ? face.getSimpleName() : "Entity") + "{uuid=" + e.getUniqueId() + "}";
            }
            if (proxy instanceof org.bukkit.World w) return "CraftWorld{key=" + w.getKey() + "}";
            if (proxy instanceof org.bukkit.Chunk c) return "CraftChunk{x=" + c.getX() + ", z=" + c.getZ() + "}";
            if (proxy instanceof org.bukkit.block.data.BlockData d) return "CraftBlockData{" + d.getAsString() + "}";
            if (proxy instanceof org.bukkit.block.Block b) return block(b);
            if (proxy instanceof org.bukkit.command.ConsoleCommandSender) return named("com.destroystokyo.paper.console.TerminalConsoleCommandSender", proxy);
            if (proxy instanceof org.bukkit.scoreboard.Scoreboard) return named("org.bukkit.craftbukkit.scoreboard.CraftScoreboard", proxy);
            if (proxy instanceof org.bukkit.scoreboard.Objective) return named("org.bukkit.craftbukkit.scoreboard.CraftObjective", proxy);
            if (proxy instanceof org.bukkit.scoreboard.Team) return named("org.bukkit.craftbukkit.scoreboard.CraftTeam", proxy);
            if (proxy instanceof org.bukkit.scoreboard.Score) return named("org.bukkit.craftbukkit.scoreboard.CraftScore", proxy);
            if (proxy instanceof org.bukkit.scoreboard.ScoreboardManager) return named("org.bukkit.craftbukkit.scoreboard.CraftScoreboardManager", proxy);
        } catch (RuntimeException ignored) {
            // the server's own text, below
        }
        return null;
    }

    static String named(String cls, Object o) {
        return cls + "@" + Integer.toHexString(System.identityHashCode(o));
    }

    /** CraftBlock.toString: `CraftBlock{pos=BlockPos{x=0, y=48, z=0}, type=GRAVEL, data=Block{minecraft:gravel}, fluid=...}`. */
    static String block(org.bukkit.block.Block b) {
        Material type = b.getType();
        org.bukkit.block.data.BlockData data = b.getBlockData();
        return "CraftBlock{pos=BlockPos{x=" + b.getX() + ", y=" + b.getY() + ", z=" + b.getZ() + "}, type=" + type
                + ", data=Block{" + type.getKey() + "}, fluid=net.minecraft.world.level.material." + fluid(type, data) + "@" + Integer.toHexString(System.identityHashCode(type)) + "}";
    }

    /** The game's fluid class of a block's fluid state. */
    static String fluid(Material type, org.bukkit.block.data.BlockData data) {
        switch (type) {
            case WATER, LAVA -> {
                int level = data instanceof org.bukkit.block.data.Levelled l ? l.getLevel() : 0;
                return (type == Material.WATER ? "WaterFluid$" : "LavaFluid$") + (level == 0 ? "Source" : "Flowing");
            }
            case BUBBLE_COLUMN, KELP, KELP_PLANT, SEAGRASS, TALL_SEAGRASS -> {
                return "WaterFluid$Source";
            }
            default -> {
                if (data instanceof org.bukkit.block.data.Waterlogged w && w.isWaterlogged()) return "WaterFluid$Source";
                return "EmptyFluid";
            }
        }
    }
}
