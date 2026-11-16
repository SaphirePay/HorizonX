package horizon.javalayer;

import java.util.HashMap;
import org.bukkit.Material;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.ItemStack;

/**
 * CraftInventory's queries over an inventory's contents (contains, containsAtLeast, all, first, clear(slot),
 * removeItemAnySlot), as Paper answers them: `ItemStack.equals` is the exact stack with its amount, `isSimilar`
 * the stack without it, a Material any stack of that type. Shapes.NONE for a form this doesn't answer.
 */
final class InventoryQueries {
    private InventoryQueries() {}

    static Object answer(Inventory inv, String name, Object[] args) {
        int n = args == null ? 0 : args.length;
        switch (name) {
            case "contains" -> {
                if (n == 1 && args[0] instanceof Material m) return amountOf(inv, m) > 0;
                if (n == 2 && args[0] instanceof Material m && args[1] instanceof Integer amount) return amount <= 0 || amountOf(inv, m) >= amount;
                if (n == 1 && (args[0] == null || args[0] instanceof ItemStack)) {
                    ItemStack item = (ItemStack) args[0];
                    if (item == null) return false;
                    for (ItemStack s : contents(inv)) if (item.equals(s)) return true;
                    return false;
                }
                if (n == 2 && (args[0] == null || args[0] instanceof ItemStack) && args[1] instanceof Integer amount) {
                    ItemStack item = (ItemStack) args[0];
                    if (item == null) return false;
                    if (amount <= 0) return true;
                    int found = 0;
                    for (ItemStack s : contents(inv)) if (item.equals(s) && ++found >= amount) return true;
                    return false;
                }
            }
            case "containsAtLeast" -> {
                if (n == 2 && (args[0] == null || args[0] instanceof ItemStack) && args[1] instanceof Integer amount) {
                    ItemStack item = (ItemStack) args[0];
                    if (item == null) return false;
                    if (amount <= 0) return true;
                    int sum = 0;
                    for (ItemStack s : contents(inv)) if (s != null && item.isSimilar(s) && (sum += s.getAmount()) >= amount) return true;
                    return false;
                }
            }
            case "all" -> {
                if (n == 1) {
                    HashMap<Integer, ItemStack> out = new HashMap<>();
                    ItemStack[] all = contents(inv);
                    for (int i = 0; i < all.length; i++) {
                        ItemStack s = all[i];
                        if (s == null) continue;
                        if (args[0] instanceof Material m ? s.getType() == m : args[0] instanceof ItemStack item && item.equals(s)) out.put(i, s);
                    }
                    return out;
                }
            }
            case "first" -> {
                if (n == 1) {
                    ItemStack[] all = contents(inv);
                    for (int i = 0; i < all.length; i++) {
                        ItemStack s = all[i];
                        if (s == null) continue;
                        if (args[0] instanceof Material m ? s.getType() == m : args[0] instanceof ItemStack item && item.equals(s)) return i;
                    }
                    return -1;
                }
            }
            case "clear" -> {
                if (n == 1 && args[0] instanceof Integer slot) {
                    inv.setItem(slot, null);
                    return null;
                }
            }
            case "removeItemAnySlot" -> {
                // Any slot, the storage-only limit lifted: the same as removeItem over the whole inventory.
                if (n == 1 && args[0] instanceof ItemStack[] items) return inv.removeItem(items);
            }
            default -> {
            }
        }
        return Shapes.NONE;
    }

    private static int amountOf(Inventory inv, Material m) {
        int sum = 0;
        for (ItemStack s : contents(inv)) if (s != null && s.getType() == m) sum += s.getAmount();
        return sum;
    }

    private static ItemStack[] contents(Inventory inv) {
        ItemStack[] all = inv.getContents();
        return all == null ? new ItemStack[0] : all;
    }
}
