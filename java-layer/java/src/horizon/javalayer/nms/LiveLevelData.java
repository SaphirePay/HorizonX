package horizon.javalayer.nms;

import net.minecraft.core.BlockPos;
import net.minecraft.core.GlobalPos;
import net.minecraft.resources.ResourceKey;
import net.minecraft.world.Difficulty;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.level.storage.ServerLevelData;

/**
 * A hollow ServerLevel's level data (Level.getLevelData()): read from the Bukkit world each time, so a plugin holding
 * it sees the world as it is now (its spawn, time, difficulty), and what it sets goes to the world (Pl3xMap reads
 * the respawn point on enable).
 */
public final class LiveLevelData implements ServerLevelData {
    private final org.bukkit.World world;
    private final ResourceKey<Level> dimension;

    LiveLevelData(org.bukkit.World world, ResourceKey<Level> dimension) {
        this.world = world;
        this.dimension = dimension;
    }

    @Override
    public LevelData.RespawnData getRespawnData() {
        org.bukkit.Location l = world.getSpawnLocation();
        return new LevelData.RespawnData(GlobalPos.of(dimension, new BlockPos(l.getBlockX(), l.getBlockY(), l.getBlockZ())), l.getYaw(), l.getPitch());
    }

    @Override
    public void setSpawn(LevelData.RespawnData data) {
        BlockPos p = data.pos();
        world.setSpawnLocation(new org.bukkit.Location(world, p.getX(), p.getY(), p.getZ(), data.yaw(), data.pitch()));
    }

    @Override public long getGameTime() { return world.getFullTime(); }
    @Override public void setGameTime(long time) { world.setFullTime(time); }
    @Override public boolean isHardcore() { return world.isHardcore(); }
    @Override public Difficulty getDifficulty() { return Difficulty.byId(world.getDifficulty().getValue()); }
    @Override public boolean isDifficultyLocked() { return false; }
    @Override public String getLevelName() { return world.getName(); }

    @SuppressWarnings("deprecation")
    @Override
    public GameType getGameType() {
        return GameType.byId(org.bukkit.Bukkit.getDefaultGameMode().getValue());
    }

    @SuppressWarnings("deprecation")
    @Override
    public void setGameType(GameType type) {
        org.bukkit.GameMode mode = org.bukkit.GameMode.getByValue(type.getId());
        if (mode != null) org.bukkit.Bukkit.setDefaultGameMode(mode);
    }

    @Override public boolean isInitialized() { return true; }
    @Override public void setInitialized(boolean initialized) {}
    @Override public boolean isAllowCommands() { return true; }
    @Override public void setAllowCommands(boolean allow) {}
}
