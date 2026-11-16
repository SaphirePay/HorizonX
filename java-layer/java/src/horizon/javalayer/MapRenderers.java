package horizon.javalayer;

import org.bukkit.Bukkit;
import org.bukkit.entity.Player;
import org.bukkit.map.MapCanvas;
import org.bukkit.map.MapCursor;
import org.bukkit.map.MapCursorCollection;
import org.bukkit.map.MapFont;
import org.bukkit.map.MapPalette;
import org.bukkit.map.MapRenderer;
import org.bukkit.map.MapView;

import java.awt.Color;
import java.awt.Graphics2D;
import java.awt.Image;
import java.awt.image.BufferedImage;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.CopyOnWriteArrayList;

/**
 * A map's renderers (MapView.getRenderers/addRenderer/removeRenderer), by the map's key, and their drawing: each Java
 * renderer is also handed to the SDK's map loop as a Rust adapter (natives `$mapRendererAdd`), which asks it to draw
 * (`$map` in Bridge.invokeRef) every tick for each player holding the map, as CraftMapView.render does. The canvas is
 * CraftMapCanvas's: 128x128 bytes starting at -1 (nothing drawn), one per renderer, per player when contextual, kept
 * between renders (a renderer that draws once stays drawn).
 */
public final class MapRenderers {
    private MapRenderers() {}

    private static final Map<String, List<MapRenderer>> BY_MAP = new ConcurrentHashMap<>();
    private static final Map<MapRenderer, Map<UUID, Canvas>> CANVASES = java.util.Collections.synchronizedMap(new java.util.IdentityHashMap<>());
    private static final UUID SHARED = new UUID(0, 0);

    private static String key(H h) {
        Object id = Native.tryCall(h, "getId", new Object[0]);
        return id == Native.MISSING ? (h.key.isEmpty() ? "#" + h.id : h.key) : String.valueOf(id);
    }

    /** The game's own renderer (CraftMapRenderer: the map's terrain and markers), first in every map's list as on
     *  Paper. Taking it off (MapView.removeRenderer) turns the server's off for that map. */
    static final class GameRenderer extends MapRenderer {
        GameRenderer() { super(false); }
        @Override public void render(MapView map, MapCanvas canvas, Player player) {}
        @Override public String toString() { return "org.bukkit.craftbukkit.map.CraftMapRenderer@" + Integer.toHexString(System.identityHashCode(this)); }
    }

    private static List<MapRenderer> list(H h) {
        return BY_MAP.computeIfAbsent(key(h), k -> new CopyOnWriteArrayList<>(List.of(new GameRenderer())));
    }

    static List<MapRenderer> of(H h) {
        return new ArrayList<>(list(h));
    }

    static void add(H h, MapView view, MapRenderer r) {
        List<MapRenderer> list = list(h);
        if (list.contains(r)) return;
        list.add(r);
        if (r instanceof GameRenderer) return;
        // CraftMapView.addRenderer: initialize(this) as it is added.
        try {
            r.initialize(view);
        } catch (Throwable e) {
            Logs.severe("[JavaLayer] map renderer " + r.getClass().getName() + ".initialize", e);
        }
        Native.tryCall(h, "$mapRendererAdd", Conv.args(new Object[]{r, System.identityHashCode(r), r.isContextual()}));
    }

    static boolean remove(H h, MapRenderer r) {
        List<MapRenderer> list = list(h);
        boolean removed = list.remove(r);
        if (removed && r instanceof GameRenderer) {
            Native.tryCall(h, "$mapGameRendererOff", new Object[0]);
        } else if (removed) {
            Native.tryCall(h, "$mapRendererRemove", Conv.args(new Object[]{System.identityHashCode(r)}));
            CANVASES.remove(r);
        }
        return removed;
    }

    /** `$map` from the Rust adapter: [map id, player, base (16384 chars, one byte each)] -> [16384 chars, cursors]. */
    static Object render(MapRenderer r, Object[] args) {
        int id = ((Number) args[0]).intValue();
        Player player = (Player) Conv.fromNative(args[1], Player.class);
        String base = String.valueOf(args[2]);
        @SuppressWarnings("deprecation")
        MapView view = Bukkit.getMap(id);
        if (view == null || player == null) return null;
        UUID who = r.isContextual() ? player.getUniqueId() : SHARED;
        Canvas canvas = CANVASES.computeIfAbsent(r, k -> new ConcurrentHashMap<>()).computeIfAbsent(who, k -> new Canvas(view));
        byte[] b = new byte[128 * 128];
        for (int i = 0; i < b.length && i < base.length(); i++) b[i] = (byte) base.charAt(i);
        canvas.base = b;
        r.render(view, canvas, player);
        char[] out = new char[128 * 128];
        for (int i = 0; i < out.length; i++) out[i] = (char) (canvas.buffer[i] & 0xFF);
        List<Object> cursors = new ArrayList<>();
        MapCursorCollection cc = canvas.cursors;
        for (int i = 0; i < cc.size(); i++) {
            MapCursor c = cc.getCursor(i);
            @SuppressWarnings("deprecation")
            String type = c.getType().getKey().getKey();
            String caption = c.caption() == null ? null : net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection().serialize(c.caption());
            cursors.add(Arrays.asList((int) c.getX(), (int) c.getY(), (int) c.getDirection(), type, c.isVisible(), caption));
        }
        return Conv.toNative(Arrays.asList(new String(out), cursors));
    }

    /** CraftMapCanvas, as Paper 26.3 has it (read from its classes): same bounds, colours, image and text drawing. */
    static final class Canvas implements MapCanvas {
        final byte[] buffer = new byte[128 * 128];
        final MapView view;
        byte[] base = new byte[128 * 128];
        MapCursorCollection cursors = new MapCursorCollection();

        Canvas(MapView view) {
            this.view = view;
            Arrays.fill(buffer, (byte) -1);
        }

        @Override public MapView getMapView() { return view; }
        @Override public MapCursorCollection getCursors() { return cursors; }
        @Override public void setCursors(MapCursorCollection cursors) { this.cursors = cursors; }

        @SuppressWarnings("deprecation")
        @Override public void setPixelColor(int x, int y, Color color) { setPixel(x, y, color == null ? -1 : MapPalette.matchColor(color)); }

        @SuppressWarnings("deprecation")
        @Override public Color getPixelColor(int x, int y) {
            byte b = getPixel(x, y);
            return b == -1 ? null : MapPalette.getColor(b);
        }

        @SuppressWarnings("deprecation")
        @Override public Color getBasePixelColor(int x, int y) { return MapPalette.getColor(getBasePixel(x, y)); }

        @Override public void setPixel(int x, int y, byte color) {
            if (x < 0 || y < 0 || x >= 128 || y >= 128) return;
            buffer[y * 128 + x] = color;
        }

        @Override public byte getPixel(int x, int y) {
            if (x < 0 || y < 0 || x >= 128 || y >= 128) return 0;
            return buffer[y * 128 + x];
        }

        @Override public byte getBasePixel(int x, int y) {
            if (x < 0 || y < 0 || x >= 128 || y >= 128) return 0;
            return base[y * 128 + x];
        }

        @SuppressWarnings("deprecation")
        @Override public void drawImage(int x, int y, Image image) {
            int w = image.getWidth(null), hgt = image.getHeight(null);
            int sx = Math.max(-x, 0), sy = Math.max(-y, 0), dx = Math.max(x, 0), dy = Math.max(y, 0);
            int cw = Math.min(w - sx, 128 - dx), ch = Math.min(hgt - sy, 128 - dy);
            if (cw <= 0 || ch <= 0) return;
            BufferedImage part;
            if (w >= cw && image instanceof BufferedImage bi) {
                part = (w > cw || hgt > ch) ? bi.getSubimage(sx, sy, cw, ch) : bi;
            } else {
                part = new BufferedImage(cw, ch, BufferedImage.TYPE_INT_ARGB);
                Graphics2D g = part.createGraphics();
                g.drawImage(image, 0, 0, null);
                g.dispose();
            }
            byte[] bytes = MapPalette.imageToBytes(part);
            if (x == 0 && cw == 128) {
                System.arraycopy(bytes, 0, buffer, dy * cw, cw * ch);
            } else {
                for (int row = 0; row < ch; row++) System.arraycopy(bytes, row * cw, buffer, (dy + row) * 128 + dx, cw);
            }
        }

        @Override public void drawText(int x, int y, MapFont font, String text) {
            int xStart = x;
            byte color = 44;
            com.google.common.base.Preconditions.checkArgument(font.isValid(text), "text (%s) contains invalid characters", text);
            for (int i = 0; i < text.length(); i++) {
                char c = text.charAt(i);
                if (c == '\n') {
                    x = xStart;
                    y += font.getHeight() + 1;
                    continue;
                }
                if (c == '§') {
                    int j = text.indexOf(';', i);
                    com.google.common.base.Preconditions.checkArgument(j >= 0, "text (%s) unterminated color string", text);
                    try {
                        color = Byte.parseByte(text.substring(i + 1, j));
                        i = j;
                        continue;
                    } catch (NumberFormatException ignored) {
                        // Paper draws the character then, as any other.
                    }
                }
                MapFont.CharacterSprite sprite = font.getChar(text.charAt(i));
                for (int r = 0; r < font.getHeight(); r++) {
                    for (int col = 0; col < sprite.getWidth(); col++) {
                        if (sprite.get(r, col)) setPixel(x + col, y + r, color);
                    }
                }
                x += sprite.getWidth() + 1;
            }
        }
    }
}
