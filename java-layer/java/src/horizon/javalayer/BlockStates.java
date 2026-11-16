package horizon.javalayer;

import org.bukkit.Material;
import org.bukkit.block.*;

import java.util.ArrayList;
import java.util.List;

/**
 * The BlockState interface of a block, by its type, as CraftBukkit's CraftBlockStates picks the class: a furnace's
 * state is a Furnace, a sign's a Sign... (the server hands every block entity over as one TileState kind).
 */
public final class BlockStates {
    private BlockStates() {}

    /** A BlockState not placed anywhere (a falling block's): its block data, type and location, as a snapshot. */
    static org.bukkit.block.BlockState detached(org.bukkit.block.data.BlockData data, org.bukkit.Location at) {
        java.util.List<Class<?>> faces = new ArrayList<>(List.of(org.bukkit.block.BlockState.class));
        for (Class<?> f : faces(data.getMaterial())) if (!faces.contains(f)) faces.add(f);
        org.bukkit.block.data.BlockData[] held = {data};
        return (org.bukkit.block.BlockState) java.lang.reflect.Proxy.newProxyInstance(BlockStates.class.getClassLoader(), faces.toArray(new Class<?>[0]), (proxy, m, args) -> switch (m.getName()) {
            case "getBlockData" -> held[0].clone();
            case "setBlockData" -> { held[0] = (org.bukkit.block.data.BlockData) args[0]; yield null; }
            case "getType" -> held[0].getMaterial();
            case "setType" -> { held[0] = ((Material) args[0]).createBlockData(); yield null; }
            case "getLocation" -> args == null || args.length == 0 ? at.clone() : ((org.bukkit.Location) args[0]).set(at.getX(), at.getY(), at.getZ());
            case "getWorld" -> at.getWorld();
            case "getX" -> at.getBlockX();
            case "getY" -> at.getBlockY();
            case "getZ" -> at.getBlockZ();
            case "isPlaced" -> false;
            case "update" -> false;
            case "copy" -> detached(held[0].clone(), at);
            case "hashCode" -> System.identityHashCode(proxy);
            case "equals" -> proxy == args[0];
            case "toString" -> "BlockState{" + held[0].getAsString() + "}";
            default -> m.getReturnType() == boolean.class ? false : m.getReturnType().isPrimitive() ? Conv.zero(m.getReturnType()) : null;
        });
    }

    /** The inventory type of a block's inventory. */
    static org.bukkit.event.inventory.InventoryType inventoryType(Material m) {
        if (m == null) return null;
        String n = m.name();
        if (n.endsWith("SHULKER_BOX")) return org.bukkit.event.inventory.InventoryType.SHULKER_BOX;
        return switch (n) {
            case "FURNACE" -> org.bukkit.event.inventory.InventoryType.FURNACE;
            case "BLAST_FURNACE" -> org.bukkit.event.inventory.InventoryType.BLAST_FURNACE;
            case "SMOKER" -> org.bukkit.event.inventory.InventoryType.SMOKER;
            case "BREWING_STAND" -> org.bukkit.event.inventory.InventoryType.BREWING;
            case "CHEST", "TRAPPED_CHEST" -> org.bukkit.event.inventory.InventoryType.CHEST;
            case "BARREL" -> org.bukkit.event.inventory.InventoryType.BARREL;
            case "HOPPER" -> org.bukkit.event.inventory.InventoryType.HOPPER;
            case "DISPENSER" -> org.bukkit.event.inventory.InventoryType.DISPENSER;
            case "DROPPER" -> org.bukkit.event.inventory.InventoryType.DROPPER;
            case "LECTERN" -> org.bukkit.event.inventory.InventoryType.LECTERN;
            case "JUKEBOX" -> org.bukkit.event.inventory.InventoryType.JUKEBOX;
            case "CRAFTER" -> org.bukkit.event.inventory.InventoryType.CRAFTER;
            case "CHISELED_BOOKSHELF" -> org.bukkit.event.inventory.InventoryType.CHISELED_BOOKSHELF;
            case "DECORATED_POT" -> org.bukkit.event.inventory.InventoryType.DECORATED_POT;
            case "BEACON" -> org.bukkit.event.inventory.InventoryType.BEACON;
            default -> null;
        };
    }

    /** The Inventory interface of an inventory type, as CraftBukkit's inventories are. */
    static List<Class<?>> inventoryFaces(org.bukkit.event.inventory.InventoryType t) {
        List<Class<?>> out = new ArrayList<>();
        switch (t.name()) {
            case "FURNACE", "BLAST_FURNACE", "SMOKER" -> out.add(org.bukkit.inventory.FurnaceInventory.class);
            case "BREWING" -> out.add(org.bukkit.inventory.BrewerInventory.class);
            case "ANVIL" -> out.add(org.bukkit.inventory.AnvilInventory.class);
            case "BEACON" -> out.add(org.bukkit.inventory.BeaconInventory.class);
            case "ENCHANTING" -> out.add(org.bukkit.inventory.EnchantingInventory.class);
            case "MERCHANT" -> out.add(org.bukkit.inventory.MerchantInventory.class);
            case "WORKBENCH", "CRAFTING" -> out.add(org.bukkit.inventory.CraftingInventory.class);
            case "PLAYER" -> out.add(org.bukkit.inventory.PlayerInventory.class);
            case "GRINDSTONE" -> out.add(org.bukkit.inventory.GrindstoneInventory.class);
            case "LECTERN" -> out.add(org.bukkit.inventory.LecternInventory.class);
            case "LOOM" -> out.add(org.bukkit.inventory.LoomInventory.class);
            case "CARTOGRAPHY" -> out.add(org.bukkit.inventory.CartographyInventory.class);
            case "SMITHING" -> out.add(org.bukkit.inventory.SmithingInventory.class);
            case "STONECUTTER" -> out.add(org.bukkit.inventory.StonecutterInventory.class);
            case "JUKEBOX" -> out.add(org.bukkit.inventory.JukeboxInventory.class);
            case "CRAFTER" -> out.add(org.bukkit.inventory.CrafterInventory.class);
            case "CHISELED_BOOKSHELF" -> out.add(org.bukkit.inventory.ChiseledBookshelfInventory.class);
            case "DECORATED_POT" -> out.add(org.bukkit.inventory.DecoratedPotInventory.class);
            default -> {}
        }
        return out;
    }

    static List<Class<?>> faces(Material m) {
        List<Class<?>> out = new ArrayList<>();
        if (m == null) return out;
        String n = m.name();
        if (n.endsWith("_HANGING_SIGN")) out.add(HangingSign.class);
        if (n.endsWith("_SIGN")) out.add(Sign.class);
        if (n.endsWith("SHULKER_BOX")) out.add(ShulkerBox.class);
        if (n.endsWith("_BANNER")) out.add(Banner.class);
        if (n.endsWith("_HEAD") || n.endsWith("_SKULL")) out.add(Skull.class);
        if (n.endsWith("_BED")) out.add(Bed.class);
        if (n.endsWith("COMMAND_BLOCK")) out.add(CommandBlock.class);
        if (n.endsWith("CAMPFIRE")) out.add(Campfire.class);
        if (n.endsWith("_SHELF")) out.add(Shelf.class);
        switch (n) {
            case "FURNACE" -> out.add(Furnace.class);
            case "BLAST_FURNACE" -> out.add(BlastFurnace.class);
            case "SMOKER" -> out.add(Smoker.class);
            case "CHEST", "TRAPPED_CHEST" -> out.add(Chest.class);
            case "ENDER_CHEST" -> out.add(EnderChest.class);
            case "BARREL" -> out.add(Barrel.class);
            case "HOPPER" -> out.add(Hopper.class);
            case "DISPENSER" -> out.add(Dispenser.class);
            case "DROPPER" -> out.add(Dropper.class);
            case "BREWING_STAND" -> out.add(BrewingStand.class);
            case "BEACON" -> out.add(Beacon.class);
            case "SPAWNER" -> out.add(CreatureSpawner.class);
            case "BEEHIVE", "BEE_NEST" -> out.add(Beehive.class);
            case "LECTERN" -> out.add(Lectern.class);
            case "JUKEBOX" -> out.add(Jukebox.class);
            case "BELL" -> out.add(Bell.class);
            case "ENCHANTING_TABLE" -> out.add(EnchantingTable.class);
            case "END_GATEWAY" -> out.add(EndGateway.class);
            case "CONDUIT" -> out.add(Conduit.class);
            case "COMPARATOR" -> out.add(Comparator.class);
            case "DAYLIGHT_DETECTOR" -> out.add(DaylightDetector.class);
            case "STRUCTURE_BLOCK" -> out.add(Structure.class);
            case "JIGSAW" -> out.add(Jigsaw.class);
            case "CHISELED_BOOKSHELF" -> out.add(ChiseledBookshelf.class);
            case "DECORATED_POT" -> out.add(DecoratedPot.class);
            case "CRAFTER" -> out.add(Crafter.class);
            case "TRIAL_SPAWNER" -> out.add(TrialSpawner.class);
            case "VAULT" -> out.add(Vault.class);
            case "SCULK_SENSOR" -> out.add(SculkSensor.class);
            case "CALIBRATED_SCULK_SENSOR" -> out.add(CalibratedSculkSensor.class);
            case "SCULK_CATALYST" -> out.add(SculkCatalyst.class);
            case "SCULK_SHRIEKER" -> out.add(SculkShrieker.class);
            case "BRUSHABLE_BLOCK", "SUSPICIOUS_SAND", "SUSPICIOUS_GRAVEL" -> out.add(BrushableBlock.class);
            case "CREAKING_HEART" -> out.add(CreakingHeart.class);
            default -> {}
        }
        return out;
    }
}
