package org.bukkit.craftbukkit.damage;

import org.bukkit.Location;
import org.bukkit.damage.DamageSource;
import org.bukkit.damage.DamageType;
import org.bukkit.entity.Entity;

/**
 * Paper's CraftDamageSource, under its own name (plugins and the probes read the class): the layer's damage
 * source, a value made by DamageSource.builder or by the layer for a damage event (horizon.javalayer.DamageSources).
 */
public final class CraftDamageSource implements DamageSource {
    private final DamageType type;
    private final Entity causing;
    private final Entity direct;
    private final Location location;
    private final net.kyori.adventure.pointer.Pointers context;

    public CraftDamageSource(DamageType type, Entity causing, Entity direct, Location location, net.kyori.adventure.pointer.Pointers context) {
        this.type = type;
        this.causing = causing;
        this.direct = direct;
        this.location = location;
        this.context = context;
    }

    @Override public DamageType getDamageType() { return type; }
    @Override public Entity getCausingEntity() { return causing != null ? causing : direct; }
    @Override public Entity getDirectEntity() { return direct != null ? direct : causing; }
    @Override public Location getDamageLocation() { return location; }
    @Override public Location getSourceLocation() { return location != null ? location : direct != null ? direct.getLocation() : null; }
    @Override public boolean isIndirect() { return causing != null && direct != null && causing != direct; }
    @Override public net.kyori.adventure.pointer.Pointers getDamageContext() { return context != null ? context : net.kyori.adventure.pointer.Pointers.empty(); }

    /** As the game's DamageSource: the type's exhaustion. */
    @Override public float getFoodExhaustion() { return type.getExhaustion(); }

    /** As the game's DamageSource: the type's scaling with the causing entity. */
    @Override
    public boolean scalesWithDifficulty() {
        return switch (type.getDamageScaling()) {
            case ALWAYS -> true;
            case WHEN_CAUSED_BY_LIVING_NON_PLAYER -> {
                Entity c = getCausingEntity();
                yield c instanceof org.bukkit.entity.LivingEntity && !(c instanceof org.bukkit.entity.Player);
            }
            default -> false;
        };
    }
}
