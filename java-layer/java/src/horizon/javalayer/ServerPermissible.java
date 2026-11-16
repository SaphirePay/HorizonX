package horizon.javalayer;

import org.bukkit.permissions.Permission;
import org.bukkit.permissions.PermissibleBase;
import org.bukkit.permissions.ServerOperator;

/**
 * A player's starting Permissible (CraftBukkit's `perm`): the server answers its permission checks, and its
 * attachments are Bukkit's, so a plugin that replaces it (LuckPerms) finds what it expects to copy.
 */
public final class ServerPermissible extends PermissibleBase {
    private final H h;

    public ServerPermissible(ServerOperator player, H h) {
        super(player);
        this.h = h;
    }

    private Boolean server(String name, String permission) {
        Object raw = Native.tryCall(h, name, new Object[]{permission});
        return raw instanceof Boolean b ? b : null;
    }

    @Override
    public boolean hasPermission(String permission) {
        if (super.isPermissionSet(permission)) return super.hasPermission(permission);
        Boolean answer = server("hasPermission", permission);
        return answer != null ? answer : super.hasPermission(permission);
    }

    @Override
    public boolean hasPermission(Permission permission) {
        return hasPermission(permission.getName());
    }

    @Override
    public boolean isPermissionSet(String permission) {
        if (super.isPermissionSet(permission)) return true;
        Boolean answer = server("isPermissionSet", permission);
        return answer != null && answer;
    }

    @Override
    public boolean isPermissionSet(Permission permission) {
        return isPermissionSet(permission.getName());
    }
}
