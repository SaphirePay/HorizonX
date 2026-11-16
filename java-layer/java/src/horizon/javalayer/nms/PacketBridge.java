package horizon.javalayer.nms;

import io.netty.buffer.ByteBuf;
import io.netty.channel.Channel;
import io.netty.channel.ChannelDuplexHandler;
import io.netty.channel.embedded.EmbeddedChannel;
import io.netty.util.ReferenceCountUtil;

import java.lang.reflect.Method;
import java.util.UUID;

/**
 * A player's Netty channel as packet libraries see it (handle -> connection -> channel).
 *
 * With PacketEvents on the server, the channel has vanilla's handler names (splitter, decoder, prepender, encoder,
 * packet_handler) and PacketEvents' own late injection (ServerConnectionInitializer.initChannel in PLAY) gives the
 * player its User and its encoder/decoder: what PacketEvents writes to the player (sendPacket, its wrappers) reaches
 * the channel's head as the packet's bytes and goes to the player through the Rust server.
 *
 * Without it (or if that injection fails), an embedded channel: the libraries treat it as a player without a socket
 * of its own, as Paper's NPC players, instead of kicking the player for a channel they could not find.
 */
public final class PacketBridge {
    private PacketBridge() {}

    /** The channel for a player who just got a handle. */
    public static Channel channel(UUID player) {
        HorizonChannel channel = new HorizonChannel(player);
        Object state = packetEventsPlay();
        if (state != null) {
            try {
                INIT.invoke(null, channel, state);
            } catch (ReflectiveOperationException | RuntimeException | LinkageError e) {
                // PacketEvents kicks a player whose channel it could not take unless the channel is one it skips.
                horizon.javalayer.Native.log(1, "[JavaLayer] PacketEvents could not take " + player + "'s channel: " + e);
                channel.close();
                return new EmbeddedChannel();
            }
        }
        CHANNELS.put(player, channel);
        return channel;
    }

    /**
     * Each second (Bridge's tick): once any player's channel has a handler of a plugin's (PacketEvents', AntiXray's,
     * ProtocolLib's), the game's packets go through the channels from then on (`raw:out` / `raw:in`).
     */
    /**
     * Each server tick: what was put on a player's channel's event loop from another thread (ProtocolLib hands a
     * packet its async listeners held, Orebfuscator's chunks, back that way) runs now. The channel is Netty's embedded
     * one, whose loop runs its tasks only when asked; a task's packet that reaches the head goes to the player.
     */
    public static void tick() {
        for (HorizonChannel channel : CHANNELS.values()) {
            if (!channel.isOpen() || !channel.lock.tryLock()) continue;
            try {
                channel.runPendingTasks();
            } catch (RuntimeException e) {
                horizon.javalayer.Native.log(1, "[JavaLayer] a task on " + channel.player + "'s channel failed: " + e);
            } finally {
                channel.lock.unlock();
            }
        }
    }

    public static void check() {
        for (HorizonChannel channel : CHANNELS.values()) {
            // What plugins gave the channel's event loop (AntiXray adds its handler with eventLoop().execute): an
            // embedded channel runs it only when asked, a real one at once. Not while a writer thread has the channel:
            // its handlers may be waiting for this (the game's) thread, and the next second's check runs them.
            if (channel.lock.tryLock()) {
                try {
                    channel.runPendingTasks();
                } finally {
                    channel.lock.unlock();
                }
            }
            if (!listeningOut && channel.foreign() != Foreign.NONE) listenOut();
            if (!listeningIn && channel.inbound()) listenIn();
        }
    }

    enum Foreign { NONE, BYTES, OBJECTS }

    private static final java.util.Map<UUID, HorizonChannel> CHANNELS = new java.util.concurrent.ConcurrentHashMap<>();
    private static volatile boolean listeningOut, listeningIn;

    /**
     * The game's own packets through the players' channels from now on (the engine's `raw:out` / `raw:in`, run on
     * each connection's own writer thread, as Paper's Netty threads run PacketEvents' handlers), once PacketEvents has
     * a player. Bench (100 walking players): no plugins 11.9 TPS, PacketEvents without it 11.5, with it 10.9, chunk
     * data included; about one more core. Opt-outs in plugins/JavaLayer/config.yml `options`:
     * -Dhorizon.noRawPackets=true (none), -Dhorizon.noRawBigPackets=true (no packets over 4 KB: chunk data).
     */
    private static void listenOut() {
        if (listeningOut || Boolean.getBoolean("horizon.noRawPackets")) return;
        listeningOut = true;
        horizon.javalayer.Native.query("{\"op\":\"packet_listen\",\"name\":\"raw:out\"}");
        if (!Boolean.getBoolean("horizon.noRawBigPackets")) horizon.javalayer.Native.query("{\"op\":\"packet_listen\",\"name\":\"raw:out:big\"}");
    }

    /**
     * The players' packets through their channels (`raw:in`, on the game's thread): only once a plugin's handler reads
     * them (PacketEvents' decoder). AntiXray's handler only writes; its players' packets going through Java as well
     * cost the game's thread most of a tick with 100 players (bench 3.6 TPS).
     */
    private static void listenIn() {
        if (listeningIn || Boolean.getBoolean("horizon.noRawPackets")) return;
        listeningIn = true;
        horizon.javalayer.Native.query("{\"op\":\"packet_listen\",\"name\":\"raw:in\"}");
    }

    /** A player left: their channel goes. */
    static void forget(UUID player) {
        CHANNELS.remove(player);
        THROUGH.remove(player);
    }

    /**
     * A packet the game sends a player (its body: id and fields, hex), through the player's channel from vanilla's
     * encoder on, as Paper's would go: PacketEvents' encoder reads it, changes or cancels it. Answers what reaches the
     * channel's head: "" for the packet as it was, null for none (cancelled), else the new body (hex); more than one
     * packet coming out, the first replaces it and the rest are sent after it.
     */
    private static final java.util.concurrent.atomic.AtomicLong OUT_COUNT = new java.util.concurrent.atomic.AtomicLong();
    private static final java.util.concurrent.atomic.AtomicLong OUT_NANOS = new java.util.concurrent.atomic.AtomicLong();
    private static final java.util.concurrent.atomic.AtomicLong OUT_BYTES = new java.util.concurrent.atomic.AtomicLong();
    private static final java.util.concurrent.atomic.AtomicLong OUT_BIG = new java.util.concurrent.atomic.AtomicLong();
    private static final java.util.concurrent.atomic.AtomicLong BIG_NANOS = new java.util.concurrent.atomic.AtomicLong();
    private static final java.util.concurrent.atomic.AtomicLong OUT_CHANGED = new java.util.concurrent.atomic.AtomicLong();
    private static final java.util.concurrent.atomic.AtomicLong OUT_OBJECTS = new java.util.concurrent.atomic.AtomicLong();
    private static final java.util.concurrent.atomic.AtomicLong OUT_VIA = new java.util.concurrent.atomic.AtomicLong();
    private static final java.util.concurrent.atomic.AtomicLong OUT_EXTRA = new java.util.concurrent.atomic.AtomicLong();
    private static final java.util.concurrent.atomic.AtomicLong OUT_NONE = new java.util.concurrent.atomic.AtomicLong();
    private static final java.util.concurrent.atomic.AtomicLong CONN_SENT = new java.util.concurrent.atomic.AtomicLong();
    private static final java.util.concurrent.atomic.AtomicLong CONN_PLAIN = new java.util.concurrent.atomic.AtomicLong();
    private static final java.util.concurrent.atomic.AtomicLong THROUGH_BACK = new java.util.concurrent.atomic.AtomicLong();

    /** Packets through `out` so far and the time they took in Java (for the probes and a slow server's report). */
    public static String stats() {
        long n = OUT_COUNT.get(), ns = OUT_NANOS.get();
        return n + " packets, " + (n == 0 ? 0 : ns / n / 1000) + " us each, " + (n == 0 ? 0 : OUT_BYTES.get() / n) + " bytes each, " + OUT_BIG.get() + " over 4 KB taking " + BIG_NANOS.get() / 1_000_000 + " ms of " + ns / 1_000_000 + ", " + OUT_CHANGED.get() + " changed, " + OUT_OBJECTS.get() + " as objects (" + OUT_VIA.get() + " via a plugin's channel proxy), " + OUT_EXTRA.get() + " extra, " + OUT_NONE.get() + " held; sent by the connection " + CONN_SENT.get() + " through the channel, " + CONN_PLAIN.get() + " plainly, " + THROUGH_BACK.get() + " came back; " + LiveChunks.stats();
    }

    public static String out(String uuid, String hex) {
        long started = System.nanoTime();
        try {
            return outTimed(uuid, hex);
        } finally {
            long took = System.nanoTime() - started;
            OUT_COUNT.incrementAndGet();
            OUT_NANOS.addAndGet(took);
            OUT_BYTES.addAndGet(hex.length() / 2);
            if (hex.length() > 8192) {
                OUT_BIG.incrementAndGet();
                BIG_NANOS.addAndGet(took);
            }
        }
    }

    /**
     * Bodies that already went through a player's pipeline here (a packet a plugin or ProtocolLib sent, the extra
     * packets a handler wrote): the Rust server's writer hands them to `out` like its own, and they go on as they are.
     */
    private static final java.util.Map<UUID, java.util.Map<Long, Integer>> THROUGH = new java.util.concurrent.ConcurrentHashMap<>();

    /** A body's key there: FNV-1a over its bytes, its length in the top bits. */
    private static long key(ByteBuf body) {
        long h = 0xcbf29ce484222325L;
        int at = body.readerIndex(), n = body.readableBytes();
        for (int i = 0; i < n; i++) h = (h ^ (body.getByte(at + i) & 0xFF)) * 0x100000001b3L;
        return (h & 0x0000FFFFFFFFFFFFL) ^ ((long) n << 48);
    }

    private static boolean wasThrough(UUID player, byte[] raw) {
        java.util.Map<Long, Integer> bodies = THROUGH.get(player);
        if (bodies == null || bodies.isEmpty()) return false;
        boolean[] found = {false};
        bodies.computeIfPresent(key(io.netty.buffer.Unpooled.wrappedBuffer(raw)), (k, n) -> {
            found[0] = true;
            return n > 1 ? n - 1 : null;
        });
        return found[0];
    }

    /** A body that went through the player's pipeline, to the player (it isn't run through the handlers again). */
    private static void sendThrough(UUID player, ByteBuf body) {
        // Big ones come back only when the server hands packets over 4 KB too.
        if (body.readableBytes() <= 4096 || !Boolean.getBoolean("horizon.noRawBigPackets")) {
            java.util.Map<Long, Integer> bodies = THROUGH.computeIfAbsent(player, k -> new java.util.concurrent.ConcurrentHashMap<>());
            if (bodies.size() < 4096) bodies.merge(key(body), 1, Integer::sum);
        }
        Hollow.sendBody(player, body);
    }

    /**
     * A packet object the game's Connection is asked to send (a plugin's, ProtocolLib's), as Paper's Connection
     * sends it: on the channel's event loop through the channel the Connection holds (ProtocolLib's proxy there runs
     * its listeners on the calling thread), then the pipeline; what comes out goes to the player. False when the
     * player's packets don't go through a channel here (no plugin handler in it): the caller sends it plainly.
     */
    public static boolean sendFromConnection(UUID player, net.minecraft.network.protocol.Packet<?> packet) {
        if (!listeningOut || packet instanceof net.minecraft.network.protocol.BundlePacket<?>) {
            CONN_PLAIN.incrementAndGet();
            return false;
        }
        HorizonChannel channel = CHANNELS.get(player);
        if (channel == null || !channel.isOpen() || channel.pipeline().context("encoder") == null || channel.foreign() != Foreign.OBJECTS) {
            CONN_PLAIN.incrementAndGet();
            return false;
        }
        CONN_SENT.incrementAndGet();
        java.util.List<ByteBuf> caught;
        acquire(channel);
        {
            channel.capture = new java.util.ArrayList<>();
            try {
                io.netty.channel.Channel via = Hollow.replacedChannel(player, channel);
                io.netty.channel.Channel to = via != null ? via : channel;
                to.eventLoop().execute(() -> to.writeAndFlush(packet));
                channel.runPendingTasks();
            } finally {
                caught = channel.capture;
                channel.capture = null;
                channel.lock.unlock();
            }
        }
        try {
            for (ByteBuf body : caught) sendThrough(player, body);
        } finally {
            for (ByteBuf b : caught) ReferenceCountUtil.release(b);
        }
        return true;
    }

    private static String outTimed(String uuid, String hex) {
        HorizonChannel channel = CHANNELS.get(uuidOf(uuid));
        if (channel == null || !channel.isOpen() || channel.pipeline().context("encoder") == null) return "";
        byte[] raw = bytes(hex);
        if (wasThrough(channel.player, raw)) {
            THROUGH_BACK.incrementAndGet();
            return "";
        }
        ByteBuf body = io.netty.buffer.Unpooled.wrappedBuffer(raw);
        // A handler before vanilla's encoder (AntiXray's, ProtocolLib's) reads the game's packet objects: the packet
        // made from its body goes in at the tail, as Paper's connection writes it, and the encoder makes bytes again.
        Object packet = channel.foreign() == Foreign.OBJECTS ? Hollow.decodeBody(body.duplicate()) : null;
        // A chunk as the game sends it: kept as the chunk's blocks for the plugins that read chunks (LiveChunks).
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundLevelChunkWithLightPacket chunk) LiveChunks.sent(channel.player, chunk);
        java.util.List<ByteBuf> caught;
        acquire(channel);
        {
            channel.capture = new java.util.ArrayList<>();
            try {
                if (packet != null) {
                    body.release();
                    // Through the channel the player's Connection holds, as Paper's Connection writes: a plugin's
                    // proxy there (ProtocolLib's) hears, changes or cancels it before it reaches the pipeline.
                    io.netty.channel.Channel via = Hollow.replacedChannel(channel.player, channel);
                    OUT_OBJECTS.incrementAndGet();
                    if (via != null) OUT_VIA.incrementAndGet();
                    (via != null ? via : channel).writeAndFlush(packet);
                } else {
                    channel.pipeline().context("encoder").writeAndFlush(body);
                }
                channel.runPendingTasks();
            } finally {
                caught = channel.capture;
                channel.capture = null;
                channel.lock.unlock();
            }
        }
        try {
            if (caught.isEmpty()) {
                OUT_NONE.incrementAndGet();
                return null;
            }
            OUT_EXTRA.addAndGet(caught.size() - 1);
            for (int i = 1; i < caught.size(); i++) sendThrough(channel.player, caught.get(i));
            ByteBuf first = caught.get(0);
            // Unchanged (the usual case): no new text to make.
            if (first.readableBytes() == raw.length && io.netty.buffer.ByteBufUtil.equals(first, io.netty.buffer.Unpooled.wrappedBuffer(raw))) return "";
            OUT_CHANGED.incrementAndGet();
            return hex(first);
        } finally {
            for (ByteBuf b : caught) ReferenceCountUtil.release(b);
        }
    }

    /**
     * A packet a player sent (its body, hex), through their channel from the splitter on, as Paper's would come in:
     * PacketEvents' decoder reads it, changes or cancels it. Answers what reaches vanilla's decoder: "" for the packet
     * as it was, null for none, else the new body (hex).
     */
    public static String in(String uuid, String hex) {
        HorizonChannel channel = CHANNELS.get(uuidOf(uuid));
        if (channel == null || !channel.isOpen() || channel.pipeline().context("splitter") == null) return "";
        byte[] raw = bytes(hex);
        java.util.List<ByteBuf> caught;
        acquire(channel);
        {
            channel.captureIn = new java.util.ArrayList<>();
            try {
                channel.pipeline().context("splitter").fireChannelRead(io.netty.buffer.Unpooled.wrappedBuffer(raw));
                channel.runPendingTasks();
            } finally {
                caught = channel.captureIn;
                channel.captureIn = null;
                channel.decoded.clear();
                channel.lock.unlock();
            }
        }
        try {
            if (caught.isEmpty()) return null;
            ByteBuf first = caught.get(0);
            if (first.readableBytes() == raw.length && io.netty.buffer.ByteBufUtil.equals(first, io.netty.buffer.Unpooled.wrappedBuffer(raw))) return "";
            return hex(first);
        } finally {
            for (ByteBuf b : caught) ReferenceCountUtil.release(b);
        }
    }

    /**
     * The channel's lock, for a packet through it. The game's thread (a player's packet in, `pl_receive_raw`) answers
     * the queries waiting for it while it waits: the thread holding the channel may be a writer whose plugin handler
     * asks the game something (AntiXray's player.getWorld()), and it would wait for this thread for ever.
     */
    private static void acquire(HorizonChannel channel) {
        if (channel.lock.tryLock()) return;
        while (true) {
            try {
                if (channel.lock.tryLock(1, java.util.concurrent.TimeUnit.MILLISECONDS)) return;
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                channel.lock.lock();
                return;
            }
            horizon.javalayer.Native.pumpQueries();
        }
    }

    private static UUID uuidOf(String hex32) {
        String h = hex32.replace("-", "");
        return new UUID(Long.parseUnsignedLong(h.substring(0, 16), 16), Long.parseUnsignedLong(h.substring(16), 16));
    }

    private static final char[] DIGITS = "0123456789abcdef".toCharArray();

    private static byte[] bytes(String hex) {
        byte[] out = new byte[hex.length() / 2];
        for (int i = 0; i < out.length; i++) out[i] = (byte) ((Character.digit(hex.charAt(2 * i), 16) << 4) | Character.digit(hex.charAt(2 * i + 1), 16));
        return out;
    }

    private static String hex(ByteBuf buf) {
        int n = buf.readableBytes(), at = buf.readerIndex();
        char[] out = new char[n * 2];
        for (int i = 0; i < n; i++) {
            int b = buf.getByte(at + i) & 0xFF;
            out[2 * i] = DIGITS[b >>> 4];
            out[2 * i + 1] = DIGITS[b & 0xF];
        }
        return new String(out);
    }

    private static Method INIT;
    private static Object PLAY;
    private static boolean looked;

    /** PacketEvents' ConnectionState.PLAY when its plugin is enabled (its initChannel found), else null. */
    private static synchronized Object packetEventsPlay() {
        if (looked) return INIT == null ? null : PLAY;
        org.bukkit.plugin.Plugin pe = org.bukkit.Bukkit.getPluginManager().getPlugin("packetevents");
        if (pe == null || !pe.isEnabled()) return null;
        looked = true;
        try {
            ClassLoader loader = pe.getClass().getClassLoader();
            Class<?> state = Class.forName("com.github.retrooper.packetevents.protocol.ConnectionState", true, loader);
            Class<?> init = Class.forName("io.github.retrooper.packetevents.injector.connection.ServerConnectionInitializer", true, loader);
            INIT = init.getMethod("initChannel", Object.class, state);
            for (Object c : state.getEnumConstants()) if (((Enum<?>) c).name().equals("PLAY")) PLAY = c;
        } catch (ReflectiveOperationException | LinkageError e) {
            horizon.javalayer.Native.log(1, "[JavaLayer] PacketEvents' injection is not where 2.x keeps it: " + e);
            INIT = null;
        }
        return INIT == null ? null : PLAY;
    }

    /** A player's channel with vanilla's handler names; what reaches its head goes to the player. */
    static final class HorizonChannel extends EmbeddedChannel {
        final UUID player;
        /** While the game's own packet passes (`out`): what reaches the head is kept here, not sent. */
        java.util.List<ByteBuf> capture;
        /** While a player's packet passes (`in`): what reaches vanilla's decoder is kept here. */
        java.util.List<ByteBuf> captureIn;
        /** The bytes each packet object the decoder made came from (while a player's packet passes). */
        final java.util.Map<Object, byte[]> decoded = java.util.Collections.synchronizedMap(new java.util.IdentityHashMap<>());

        HorizonChannel(UUID player) {
            this.player = player;
            for (String name : new String[]{"splitter", "decoder", "prepender", "encoder", "packet_handler"}) {
                pipeline().addLast(name, switch (name) {
                    case "decoder" -> new Decoder(this);
                    case "encoder" -> new Encoder();
                    case "packet_handler" -> new Arrived(this);
                    default -> new ChannelDuplexHandler();
                });
            }
        }

        /** Held while a packet goes through (a writer or reader thread); the game's thread only tries it. */
        final java.util.concurrent.locks.ReentrantLock lock = new java.util.concurrent.locks.ReentrantLock();

        private static final java.util.Set<String> OWN = java.util.Set.of("splitter", "decoder", "prepender", "encoder", "packet_handler");

        /** What plugins added to this channel: nothing, byte handlers only (PacketEvents'), or object handlers too. */
        Foreign foreign() {
            Foreign found = Foreign.NONE;
            for (String name : pipeline().names()) {
                if (OWN.contains(name)) continue;
                io.netty.channel.ChannelHandler handler = pipeline().get(name);
                if (handler == null || handler.getClass().getName().startsWith("io.netty.channel.DefaultChannelPipeline")) continue;
                String of = handler.getClass().getName();
                if (of.startsWith("io.github.retrooper.") || of.startsWith("com.github.retrooper.")) {
                    found = Foreign.BYTES;
                } else {
                    return Foreign.OBJECTS;
                }
            }
            return found;
        }

        /**
         * Whether a plugin's handler reads the player's packets: a decoder, or a handler whose own class (not one of
         * Netty's pass-through adapters) reads them (channelRead).
         */
        boolean inbound() {
            for (String name : pipeline().names()) {
                if (OWN.contains(name)) continue;
                io.netty.channel.ChannelHandler handler = pipeline().get(name);
                if (!(handler instanceof io.netty.channel.ChannelInboundHandler)) continue;
                if (handler instanceof io.netty.handler.codec.ByteToMessageDecoder || handler instanceof io.netty.handler.codec.MessageToMessageDecoder<?>) return true;
                try {
                    Class<?> by = handler.getClass().getMethod("channelRead", io.netty.channel.ChannelHandlerContext.class, Object.class).getDeclaringClass();
                    if (!by.getName().startsWith("io.netty.")) return true;
                } catch (NoSuchMethodException e) {
                    return true;
                }
            }
            return false;
        }

        /** Vanilla's PacketEncoder: a packet object to its body (id and fields), with the game's own codec. */
        static final class Encoder extends net.minecraft.network.PacketEncoder<net.minecraft.network.protocol.game.ClientGamePacketListener> {
            Encoder() { super(Hollow.clientbound()); }

            /**
             * The game's codec, as vanilla's encoder writes the packet: not Paper's encode around it, which starts an
             * item obfuscation session from Paper's global config (not loaded here: every entity data packet failed),
             * and swaps the protocol out after a packet ending the play phase (the Rust server keeps the connection).
             */
            @Override
            protected void encode(io.netty.channel.ChannelHandlerContext ctx, net.minecraft.network.protocol.Packet<net.minecraft.network.protocol.game.ClientGamePacketListener> packet, ByteBuf out) {
                Hollow.encodeBody(packet, out);
            }
        }

        /**
         * Vanilla's PacketDecoder (ProtocolLib finds the protocol on it, plugin handlers after it read packet
         * objects); a packet it can't read goes on as the bytes it was. The bytes of each packet it made are kept, so
         * one that reaches the game unchanged is sent on exactly as the player sent it.
         */
        static final class Decoder extends net.minecraft.network.PacketDecoder<net.minecraft.network.protocol.game.ServerGamePacketListener> {
            private final HorizonChannel channel;

            Decoder(HorizonChannel channel) {
                super(Hollow.serverbound());
                this.channel = channel;
            }

            @Override
            protected void decode(io.netty.channel.ChannelHandlerContext ctx, ByteBuf in, java.util.List<Object> out) {
                byte[] raw = io.netty.buffer.ByteBufUtil.getBytes(in, in.readerIndex(), in.readableBytes());
                int before = out.size();
                try {
                    super.decode(ctx, in, out);
                } catch (Exception | LinkageError e) {
                    while (out.size() > before) ReferenceCountUtil.release(out.remove(out.size() - 1));
                    in.skipBytes(in.readableBytes());
                    out.add(io.netty.buffer.Unpooled.wrappedBuffer(raw));
                    return;
                }
                for (int i = before; i < out.size(); i++) channel.decoded.put(out.get(i), raw);
            }
        }

        /** Where the game's packet handler sits: a player's packet that got this far is what the game handles. */
        static final class Arrived extends ChannelDuplexHandler {
            private final HorizonChannel channel;

            Arrived(HorizonChannel channel) { this.channel = channel; }

            @Override
            public void channelRead(io.netty.channel.ChannelHandlerContext ctx, Object msg) {
                try {
                    if (channel.captureIn == null) return;
                    if (msg instanceof ByteBuf buf) {
                        channel.captureIn.add(buf.retain());
                    } else if (msg instanceof net.minecraft.network.protocol.Packet<?> packet) {
                        byte[] raw = channel.decoded.get(packet);
                        if (raw != null) {
                            channel.captureIn.add(io.netty.buffer.Unpooled.wrappedBuffer(raw));
                        } else {
                            ByteBuf body = io.netty.buffer.Unpooled.buffer();
                            Hollow.encodeServerbound(packet, body);
                            channel.captureIn.add(body);
                        }
                    }
                } finally {
                    ReferenceCountUtil.release(msg);
                }
            }
        }

        @Override
        protected void handleOutboundMessage(Object msg) {
            try {
                if (msg instanceof ByteBuf buf && buf.isReadable()) {
                    if (capture != null) {
                        capture.add(buf.retain());
                    } else {
                        sendThrough(player, buf);
                    }
                }
            } finally {
                ReferenceCountUtil.release(msg);
            }
        }
    }
}
