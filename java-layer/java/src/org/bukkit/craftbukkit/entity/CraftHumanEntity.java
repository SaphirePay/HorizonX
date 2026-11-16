package org.bukkit.craftbukkit.entity;

import org.bukkit.permissions.PermissibleBase;

/**
 * CraftBukkit's HumanEntity base, for what plugins reach by name: its `perm` field is the player's Permissible, which
 * LuckPerms replaces with its own. When it is set, permission checks ask it; otherwise the server answers.
 * It extends Paper's real CraftLivingEntity (so vanilla code that expects a CraftEntity loads); objects are made
 * without constructors (Craft.allocate) and every Bukkit method is answered by the layer.
 */
public abstract class CraftHumanEntity extends CraftLivingEntity implements org.bukkit.entity.HumanEntity {
    protected PermissibleBase perm;

    protected CraftHumanEntity() {
        super(null, null);
    }

    /** Over a vanilla player a plugin made itself (an NPC CraftPlayer, horizon.javalayer.Npcs): Paper's own code. */
    protected CraftHumanEntity(org.bukkit.craftbukkit.CraftServer server, net.minecraft.world.entity.LivingEntity entity) {
        super(server, entity);
        this.perm = new PermissibleBase(this);
    }

    public PermissibleBase horizonPermissible() {
        return perm;
    }
}
