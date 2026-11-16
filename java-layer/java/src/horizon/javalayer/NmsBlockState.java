package horizon.javalayer;

/**
 * Paper's CraftBlockData.getState(): the game's own BlockState for a block data. The layer's BlockData objects carry
 * this interface so plugins that look the method up by reflection (AntiXray's packet engine, through
 * data.getClass().getMethod("getState")) find it; NativeHandler answers it with the game's state parsed from
 * getAsString().
 */
public interface NmsBlockState {
    Object getState();
}
