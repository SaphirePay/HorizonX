package com.comphenix.protocol;

import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/** ProtocolLib's packet types, on Horizon: each is the server's packet of that name (horizon.javalayer). */
public class PacketType implements Comparable<PacketType>, horizon.javalayer.Proxies.Handled {
    private static final Map<String, PacketType> ALL = new ConcurrentHashMap<>();
    private final boolean server;
    private final String name;
    private horizon.javalayer.H handle;

    public PacketType(boolean server, String name) {
        this.server = server;
        this.name = name;
        ALL.putIfAbsent((server ? "S:" : "C:") + name, this);
    }

    /** The server's own packet type object, for the bridge (Handled: the layer passes it to the server as itself:
     *  createPacket(type) returned null for every type before). */
    @Override
    public horizon.javalayer.H horizonHandle() {
        if (handle == null && horizon.javalayer.Native.field(server ? "Server" : "Client", name) instanceof horizon.javalayer.H h) handle = h;
        return handle;
    }

    public String name() { return name; }
    public boolean isServer() { return server; }
    public boolean isClient() { return !server; }
    public Sender getSender() { return server ? Sender.SERVER : Sender.CLIENT; }
    public Protocol getProtocol() { return Protocol.PLAY; }
    public boolean isSupported() { return true; }
    public String[] getClassNames() { return new String[0]; }
    public PacketType.Play getPlay() { return null; }
    @Override public String toString() { return name + "[" + (server ? "SERVER" : "CLIENT") + "]"; }
    @Override public boolean equals(Object o) { return o instanceof PacketType t && t.server == server && t.name.equals(name); }
    @Override public int hashCode() { return name.hashCode() * 31 + (server ? 1 : 0); }
    @Override public int compareTo(PacketType o) { return toString().compareTo(o.toString()); }

    public static Iterable<PacketType> values() { return new ArrayList<>(ALL.values()); }
    public static PacketType fromName(String name) { return ALL.getOrDefault("S:" + name, ALL.get("C:" + name)); }
    public static Collection<PacketType> fromName(String name, Sender sender) { PacketType t = ALL.get((sender == Sender.SERVER ? "S:" : "C:") + name); return t == null ? List.of() : List.of(t); }

    public enum Sender { CLIENT, SERVER; public Sender opposite() { return this == CLIENT ? SERVER : CLIENT; } }
    public enum Protocol { HANDSHAKING, PLAY, STATUS, LOGIN, CONFIGURATION, UNKNOWN }

    public static class Play {
        public static class Server {
            public static final PacketType SPAWN_ENTITY = new PacketType(true, "SPAWN_ENTITY");
            public static final PacketType ANIMATION = new PacketType(true, "ANIMATION");
            public static final PacketType STATISTIC = new PacketType(true, "STATISTIC");
            public static final PacketType BLOCK_BREAK_ANIMATION = new PacketType(true, "BLOCK_BREAK_ANIMATION");
            public static final PacketType TILE_ENTITY_DATA = new PacketType(true, "TILE_ENTITY_DATA");
            public static final PacketType BLOCK_ACTION = new PacketType(true, "BLOCK_ACTION");
            public static final PacketType BLOCK_CHANGE = new PacketType(true, "BLOCK_CHANGE");
            public static final PacketType BOSS = new PacketType(true, "BOSS");
            public static final PacketType SERVER_DIFFICULTY = new PacketType(true, "SERVER_DIFFICULTY");
            public static final PacketType CLEAR_TITLES = new PacketType(true, "CLEAR_TITLES");
            public static final PacketType COMMANDS = new PacketType(true, "COMMANDS");
            public static final PacketType CLOSE_WINDOW = new PacketType(true, "CLOSE_WINDOW");
            public static final PacketType WINDOW_ITEMS = new PacketType(true, "WINDOW_ITEMS");
            public static final PacketType WINDOW_DATA = new PacketType(true, "WINDOW_DATA");
            public static final PacketType SET_SLOT = new PacketType(true, "SET_SLOT");
            public static final PacketType SET_COOLDOWN = new PacketType(true, "SET_COOLDOWN");
            public static final PacketType CUSTOM_PAYLOAD = new PacketType(true, "CUSTOM_PAYLOAD");
            public static final PacketType KICK_DISCONNECT = new PacketType(true, "KICK_DISCONNECT");
            public static final PacketType ENTITY_STATUS = new PacketType(true, "ENTITY_STATUS");
            public static final PacketType UNLOAD_CHUNK = new PacketType(true, "UNLOAD_CHUNK");
            public static final PacketType GAME_STATE_CHANGE = new PacketType(true, "GAME_STATE_CHANGE");
            public static final PacketType HURT_ANIMATION = new PacketType(true, "HURT_ANIMATION");
            public static final PacketType DAMAGE_EVENT = new PacketType(true, "DAMAGE_EVENT");
            public static final PacketType KEEP_ALIVE = new PacketType(true, "KEEP_ALIVE");
            public static final PacketType MAP_CHUNK = new PacketType(true, "MAP_CHUNK");
            public static final PacketType WORLD_EVENT = new PacketType(true, "WORLD_EVENT");
            public static final PacketType LIGHT_UPDATE = new PacketType(true, "LIGHT_UPDATE");
            public static final PacketType LOGIN = new PacketType(true, "LOGIN");
            public static final PacketType MAP = new PacketType(true, "MAP");
            public static final PacketType REL_ENTITY_MOVE = new PacketType(true, "REL_ENTITY_MOVE");
            public static final PacketType REL_ENTITY_MOVE_LOOK = new PacketType(true, "REL_ENTITY_MOVE_LOOK");
            public static final PacketType ENTITY_LOOK = new PacketType(true, "ENTITY_LOOK");
            public static final PacketType OPEN_WINDOW_MERCHANT = new PacketType(true, "OPEN_WINDOW_MERCHANT");
            public static final PacketType OPEN_WINDOW = new PacketType(true, "OPEN_WINDOW");
            public static final PacketType OPEN_BOOK = new PacketType(true, "OPEN_BOOK");
            public static final PacketType OPEN_SIGN_EDITOR = new PacketType(true, "OPEN_SIGN_EDITOR");
            public static final PacketType PING = new PacketType(true, "PING");
            public static final PacketType ABILITIES = new PacketType(true, "ABILITIES");
            public static final PacketType CHAT = new PacketType(true, "CHAT");
            public static final PacketType SYSTEM_CHAT = new PacketType(true, "SYSTEM_CHAT");
            public static final PacketType PLAYER_INFO_REMOVE = new PacketType(true, "PLAYER_INFO_REMOVE");
            public static final PacketType PLAYER_INFO = new PacketType(true, "PLAYER_INFO");
            public static final PacketType LOOK_AT = new PacketType(true, "LOOK_AT");
            public static final PacketType POSITION = new PacketType(true, "POSITION");
            public static final PacketType ENTITY_DESTROY = new PacketType(true, "ENTITY_DESTROY");
            public static final PacketType REMOVE_ENTITY_EFFECT = new PacketType(true, "REMOVE_ENTITY_EFFECT");
            public static final PacketType RESPAWN = new PacketType(true, "RESPAWN");
            public static final PacketType ENTITY_HEAD_ROTATION = new PacketType(true, "ENTITY_HEAD_ROTATION");
            public static final PacketType MULTI_BLOCK_CHANGE = new PacketType(true, "MULTI_BLOCK_CHANGE");
            public static final PacketType SERVER_DATA = new PacketType(true, "SERVER_DATA");
            public static final PacketType SET_ACTION_BAR_TEXT = new PacketType(true, "SET_ACTION_BAR_TEXT");
            public static final PacketType CAMERA = new PacketType(true, "CAMERA");
            public static final PacketType VIEW_CENTRE = new PacketType(true, "VIEW_CENTRE");
            public static final PacketType SPAWN_POSITION = new PacketType(true, "SPAWN_POSITION");
            public static final PacketType SCOREBOARD_DISPLAY_OBJECTIVE = new PacketType(true, "SCOREBOARD_DISPLAY_OBJECTIVE");
            public static final PacketType ENTITY_METADATA = new PacketType(true, "ENTITY_METADATA");
            public static final PacketType ENTITY_VELOCITY = new PacketType(true, "ENTITY_VELOCITY");
            public static final PacketType ATTACH_ENTITY = new PacketType(true, "ATTACH_ENTITY");
            public static final PacketType ENTITY_EQUIPMENT = new PacketType(true, "ENTITY_EQUIPMENT");
            public static final PacketType EXPERIENCE = new PacketType(true, "EXPERIENCE");
            public static final PacketType UPDATE_HEALTH = new PacketType(true, "UPDATE_HEALTH");
            public static final PacketType HELD_ITEM_SLOT = new PacketType(true, "HELD_ITEM_SLOT");
            public static final PacketType SCOREBOARD_OBJECTIVE = new PacketType(true, "SCOREBOARD_OBJECTIVE");
            public static final PacketType MOUNT = new PacketType(true, "MOUNT");
            public static final PacketType SCOREBOARD_TEAM = new PacketType(true, "SCOREBOARD_TEAM");
            public static final PacketType SCOREBOARD_SCORE = new PacketType(true, "SCOREBOARD_SCORE");
            public static final PacketType SET_SUBTITLE_TEXT = new PacketType(true, "SET_SUBTITLE_TEXT");
            public static final PacketType UPDATE_TIME = new PacketType(true, "UPDATE_TIME");
            public static final PacketType SET_TITLE_TEXT = new PacketType(true, "SET_TITLE_TEXT");
            public static final PacketType SET_TITLES_ANIMATION = new PacketType(true, "SET_TITLES_ANIMATION");
            public static final PacketType NAMED_SOUND_EFFECT = new PacketType(true, "NAMED_SOUND_EFFECT");
            public static final PacketType ENTITY_SOUND = new PacketType(true, "ENTITY_SOUND");
            public static final PacketType STOP_SOUND = new PacketType(true, "STOP_SOUND");
            public static final PacketType PLAYER_LIST_HEADER_FOOTER = new PacketType(true, "PLAYER_LIST_HEADER_FOOTER");
            public static final PacketType COLLECT = new PacketType(true, "COLLECT");
            public static final PacketType ENTITY_TELEPORT = new PacketType(true, "ENTITY_TELEPORT");
            public static final PacketType ADVANCEMENTS = new PacketType(true, "ADVANCEMENTS");
            public static final PacketType UPDATE_ATTRIBUTES = new PacketType(true, "UPDATE_ATTRIBUTES");
            public static final PacketType WORLD_PARTICLES = new PacketType(true, "WORLD_PARTICLES");
            public static final PacketType EXPLOSION = new PacketType(true, "EXPLOSION");
            public static final PacketType ENTITY_EFFECT = new PacketType(true, "ENTITY_EFFECT");
            public static final PacketType TAGS = new PacketType(true, "TAGS");
            public static final PacketType CHUNK_DATA = new PacketType(true, "CHUNK_DATA");
            public static final PacketType DESTROY_ENTITIES = new PacketType(true, "DESTROY_ENTITIES");
            public static final PacketType ENTITY_RELATIVE_MOVE = new PacketType(true, "ENTITY_RELATIVE_MOVE");
            public static final PacketType ENTITY_RELATIVE_MOVE_AND_ROTATION = new PacketType(true, "ENTITY_RELATIVE_MOVE_AND_ROTATION");
            public static final PacketType ENTITY_ROTATION = new PacketType(true, "ENTITY_ROTATION");
            public static final PacketType ENTITY_HEAD_LOOK = new PacketType(true, "ENTITY_HEAD_LOOK");
            public static final PacketType TIME_UPDATE = new PacketType(true, "TIME_UPDATE");
            public static final PacketType SOUND_EFFECT = new PacketType(true, "SOUND_EFFECT");
            public static final PacketType PARTICLE = new PacketType(true, "PARTICLE");
            public static final PacketType CHAT_MESSAGE = new PacketType(true, "CHAT_MESSAGE");
            public static final PacketType SYSTEM_CHAT_MESSAGE = new PacketType(true, "SYSTEM_CHAT_MESSAGE");
            public static final PacketType PLAYER_INFO_UPDATE = new PacketType(true, "PLAYER_INFO_UPDATE");
            public static final PacketType DISCONNECT = new PacketType(true, "DISCONNECT");
            public static final PacketType EFFECT = new PacketType(true, "EFFECT");
            public static final PacketType PLAYER_POSITION_AND_LOOK = new PacketType(true, "PLAYER_POSITION_AND_LOOK");
            public static final PacketType BLOCK_ENTITY_DATA = new PacketType(true, "BLOCK_ENTITY_DATA");
            public static final PacketType UPDATE_LIGHT = new PacketType(true, "UPDATE_LIGHT");
            public static final PacketType JOIN_GAME = new PacketType(true, "JOIN_GAME");
            public static final PacketType WINDOW_PROPERTY = new PacketType(true, "WINDOW_PROPERTY");
            public static final PacketType BUNDLE = new PacketType(true, "BUNDLE");
            public static final PacketType BLOCK_CHANGED_ACK = new PacketType(true, "BLOCK_CHANGED_ACK");
            public static final PacketType CHUNK_BATCH_FINISHED = new PacketType(true, "CHUNK_BATCH_FINISHED");
            public static final PacketType CHUNK_BATCH_START = new PacketType(true, "CHUNK_BATCH_START");
            public static final PacketType CHUNKS_BIOMES = new PacketType(true, "CHUNKS_BIOMES");
            public static final PacketType TAB_COMPLETE = new PacketType(true, "TAB_COMPLETE");
            public static final PacketType COOKIE_REQUEST = new PacketType(true, "COOKIE_REQUEST");
            public static final PacketType CUSTOM_CHAT_COMPLETIONS = new PacketType(true, "CUSTOM_CHAT_COMPLETIONS");
            public static final PacketType DEBUG_SAMPLE = new PacketType(true, "DEBUG_SAMPLE");
            public static final PacketType DELETE_CHAT_MESSAGE = new PacketType(true, "DELETE_CHAT_MESSAGE");
            public static final PacketType DISGUISED_CHAT = new PacketType(true, "DISGUISED_CHAT");
            public static final PacketType ENTITY_POSITION_SYNC = new PacketType(true, "ENTITY_POSITION_SYNC");
            public static final PacketType OPEN_WINDOW_HORSE = new PacketType(true, "OPEN_WINDOW_HORSE");
            public static final PacketType INITIALIZE_BORDER = new PacketType(true, "INITIALIZE_BORDER");
            public static final PacketType MOVE_MINECART = new PacketType(true, "MOVE_MINECART");
            public static final PacketType VEHICLE_MOVE = new PacketType(true, "VEHICLE_MOVE");
            public static final PacketType PONG_RESPONSE = new PacketType(true, "PONG_RESPONSE");
            public static final PacketType AUTO_RECIPE = new PacketType(true, "AUTO_RECIPE");
            public static final PacketType PLAYER_COMBAT_END = new PacketType(true, "PLAYER_COMBAT_END");
            public static final PacketType PLAYER_COMBAT_ENTER = new PacketType(true, "PLAYER_COMBAT_ENTER");
            public static final PacketType PLAYER_COMBAT_KILL = new PacketType(true, "PLAYER_COMBAT_KILL");
            public static final PacketType PLAYER_ROTATION = new PacketType(true, "PLAYER_ROTATION");
            public static final PacketType RECIPE_BOOK_ADD = new PacketType(true, "RECIPE_BOOK_ADD");
            public static final PacketType RECIPE_BOOK_REMOVE = new PacketType(true, "RECIPE_BOOK_REMOVE");
            public static final PacketType RECIPE_BOOK_SETTINGS = new PacketType(true, "RECIPE_BOOK_SETTINGS");
            public static final PacketType RESET_SCORE = new PacketType(true, "RESET_SCORE");
            public static final PacketType REMOVE_RESOURCE_PACK = new PacketType(true, "REMOVE_RESOURCE_PACK");
            public static final PacketType ADD_RESOURCE_PACK = new PacketType(true, "ADD_RESOURCE_PACK");
            public static final PacketType SELECT_ADVANCEMENT_TAB = new PacketType(true, "SELECT_ADVANCEMENT_TAB");
            public static final PacketType SET_BORDER_CENTER = new PacketType(true, "SET_BORDER_CENTER");
            public static final PacketType SET_BORDER_LERP_SIZE = new PacketType(true, "SET_BORDER_LERP_SIZE");
            public static final PacketType SET_BORDER_SIZE = new PacketType(true, "SET_BORDER_SIZE");
            public static final PacketType SET_BORDER_WARNING_DELAY = new PacketType(true, "SET_BORDER_WARNING_DELAY");
            public static final PacketType SET_BORDER_WARNING_DISTANCE = new PacketType(true, "SET_BORDER_WARNING_DISTANCE");
            public static final PacketType VIEW_DISTANCE = new PacketType(true, "VIEW_DISTANCE");
            public static final PacketType SET_CURSOR_ITEM = new PacketType(true, "SET_CURSOR_ITEM");
            public static final PacketType SET_PLAYER_INVENTORY = new PacketType(true, "SET_PLAYER_INVENTORY");
            public static final PacketType UPDATE_SIMULATION_DISTANCE = new PacketType(true, "UPDATE_SIMULATION_DISTANCE");
            public static final PacketType START_CONFIGURATION = new PacketType(true, "START_CONFIGURATION");
            public static final PacketType STORE_COOKIE = new PacketType(true, "STORE_COOKIE");
            public static final PacketType NBT_QUERY = new PacketType(true, "NBT_QUERY");
            public static final PacketType TEST_INSTANCE_BLOCK_STATUS = new PacketType(true, "TEST_INSTANCE_BLOCK_STATUS");
            public static final PacketType TICKING_STATE = new PacketType(true, "TICKING_STATE");
            public static final PacketType TICKING_STEP_STATE = new PacketType(true, "TICKING_STEP_STATE");
            public static final PacketType TRANSFER = new PacketType(true, "TRANSFER");
            public static final PacketType RECIPE_UPDATE = new PacketType(true, "RECIPE_UPDATE");
            public static final PacketType PROJECTILE_POWER = new PacketType(true, "PROJECTILE_POWER");
            public static final PacketType REPORT_DETAILS = new PacketType(true, "REPORT_DETAILS");
            public static final PacketType SERVER_LINKS = new PacketType(true, "SERVER_LINKS");
            public static final PacketType TRACKED_WAYPOINT = new PacketType(true, "TRACKED_WAYPOINT");
            public static final PacketType CLEAR_DIALOG = new PacketType(true, "CLEAR_DIALOG");
            public static final PacketType SHOW_DIALOG = new PacketType(true, "SHOW_DIALOG");
            public static final PacketType MAP_CHUNK_BULK = new PacketType(true, "MAP_CHUNK_BULK");
            public static final PacketType SET_COMPRESSION = new PacketType(true, "SET_COMPRESSION");
            public static final PacketType UPDATE_ENTITY_NBT = new PacketType(true, "UPDATE_ENTITY_NBT");
            public static final PacketType CRAFT_PROGRESS_BAR = new PacketType(true, "CRAFT_PROGRESS_BAR");
            public static final PacketType ENTITY_MOVE_LOOK = new PacketType(true, "ENTITY_MOVE_LOOK");
            public static final PacketType STATISTICS = new PacketType(true, "STATISTICS");
            public static final PacketType OPEN_SIGN_ENTITY = new PacketType(true, "OPEN_SIGN_ENTITY");
            public static final PacketType UPDATE_SIGN = new PacketType(true, "UPDATE_SIGN");
            public static final PacketType BED = new PacketType(true, "BED");
            public static final PacketType USE_BED = new PacketType(true, "USE_BED");
            public static final PacketType SPAWN_ENTITY_WEATHER = new PacketType(true, "SPAWN_ENTITY_WEATHER");
            public static final PacketType TITLE = new PacketType(true, "TITLE");
            public static final PacketType WORLD_BORDER = new PacketType(true, "WORLD_BORDER");
            public static final PacketType COMBAT_EVENT = new PacketType(true, "COMBAT_EVENT");
            public static final PacketType TRANSACTION = new PacketType(true, "TRANSACTION");
            public static final PacketType ENTITY = new PacketType(true, "ENTITY");
            public static final PacketType SPAWN_ENTITY_LIVING = new PacketType(true, "SPAWN_ENTITY_LIVING");
            public static final PacketType SPAWN_ENTITY_PAINTING = new PacketType(true, "SPAWN_ENTITY_PAINTING");
            public static final PacketType ADD_VIBRATION_SIGNAL = new PacketType(true, "ADD_VIBRATION_SIGNAL");
            public static final PacketType BLOCK_BREAK = new PacketType(true, "BLOCK_BREAK");
            public static final PacketType CHAT_PREVIEW = new PacketType(true, "CHAT_PREVIEW");
            public static final PacketType PLAYER_CHAT_HEADER = new PacketType(true, "PLAYER_CHAT_HEADER");
            public static final PacketType SET_DISPLAY_CHAT_PREVIEW = new PacketType(true, "SET_DISPLAY_CHAT_PREVIEW");
            public static final PacketType CUSTOM_SOUND_EFFECT = new PacketType(true, "CUSTOM_SOUND_EFFECT");
            public static final PacketType UPDATE_ENABLED_FEATURES = new PacketType(true, "UPDATE_ENABLED_FEATURES");
            public static final PacketType NAMED_ENTITY_SPAWN = new PacketType(true, "NAMED_ENTITY_SPAWN");
            public static final PacketType RESOURCE_PACK_SEND = new PacketType(true, "RESOURCE_PACK_SEND");
            public static final PacketType RECIPES = new PacketType(true, "RECIPES");
            public static final PacketType SPAWN_ENTITY_EXPERIENCE_ORB = new PacketType(true, "SPAWN_ENTITY_EXPERIENCE_ORB");
            public static Server getInstance() { return new Server(); }
        }
        public static class Client {
            public static final PacketType INTERACT_ENTITY = new PacketType(false, "INTERACT_ENTITY");
            public static final PacketType USE_ENTITY = new PacketType(false, "USE_ENTITY");
            public static final PacketType CHAT_MESSAGE = new PacketType(false, "CHAT_MESSAGE");
            public static final PacketType CHAT = new PacketType(false, "CHAT");
            public static final PacketType CHAT_COMMAND = new PacketType(false, "CHAT_COMMAND");
            public static final PacketType PLAYER_DIGGING = new PacketType(false, "PLAYER_DIGGING");
            public static final PacketType BLOCK_DIG = new PacketType(false, "BLOCK_DIG");
            public static final PacketType PLAYER_BLOCK_PLACEMENT = new PacketType(false, "PLAYER_BLOCK_PLACEMENT");
            public static final PacketType USE_ITEM_ON = new PacketType(false, "USE_ITEM_ON");
            public static final PacketType USE_ITEM = new PacketType(false, "USE_ITEM");
            public static final PacketType BLOCK_PLACE = new PacketType(false, "BLOCK_PLACE");
            public static final PacketType ANIMATION = new PacketType(false, "ANIMATION");
            public static final PacketType ARM_ANIMATION = new PacketType(false, "ARM_ANIMATION");
            public static final PacketType CLICK_WINDOW = new PacketType(false, "CLICK_WINDOW");
            public static final PacketType WINDOW_CLICK = new PacketType(false, "WINDOW_CLICK");
            public static final PacketType CLOSE_WINDOW = new PacketType(false, "CLOSE_WINDOW");
            public static final PacketType PLAYER_POSITION = new PacketType(false, "PLAYER_POSITION");
            public static final PacketType POSITION = new PacketType(false, "POSITION");
            public static final PacketType PLAYER_POSITION_AND_ROTATION = new PacketType(false, "PLAYER_POSITION_AND_ROTATION");
            public static final PacketType POSITION_LOOK = new PacketType(false, "POSITION_LOOK");
            public static final PacketType PLAYER_ROTATION = new PacketType(false, "PLAYER_ROTATION");
            public static final PacketType LOOK = new PacketType(false, "LOOK");
            public static final PacketType PLAYER_FLYING = new PacketType(false, "PLAYER_FLYING");
            public static final PacketType FLYING = new PacketType(false, "FLYING");
            public static final PacketType ENTITY_ACTION = new PacketType(false, "ENTITY_ACTION");
            public static final PacketType HELD_ITEM_CHANGE = new PacketType(false, "HELD_ITEM_CHANGE");
            public static final PacketType HELD_ITEM_SLOT = new PacketType(false, "HELD_ITEM_SLOT");
            public static final PacketType KEEP_ALIVE = new PacketType(false, "KEEP_ALIVE");
            public static final PacketType PLUGIN_MESSAGE = new PacketType(false, "PLUGIN_MESSAGE");
            public static final PacketType CUSTOM_PAYLOAD = new PacketType(false, "CUSTOM_PAYLOAD");
            public static final PacketType TAB_COMPLETE = new PacketType(false, "TAB_COMPLETE");
            public static final PacketType UPDATE_SIGN = new PacketType(false, "UPDATE_SIGN");
            public static final PacketType PLAYER_INPUT = new PacketType(false, "PLAYER_INPUT");
            public static final PacketType STEER_VEHICLE = new PacketType(false, "STEER_VEHICLE");
            public static final PacketType CLIENT_STATUS = new PacketType(false, "CLIENT_STATUS");
            public static final PacketType CLIENT_SETTINGS = new PacketType(false, "CLIENT_SETTINGS");
            public static final PacketType SETTINGS = new PacketType(false, "SETTINGS");
            public static final PacketType PONG = new PacketType(false, "PONG");
            public static final PacketType TELEPORT_CONFIRM = new PacketType(false, "TELEPORT_CONFIRM");
            public static final PacketType CREATIVE_INVENTORY_ACTION = new PacketType(false, "CREATIVE_INVENTORY_ACTION");
            public static final PacketType SET_CREATIVE_SLOT = new PacketType(false, "SET_CREATIVE_SLOT");
            public static final PacketType TELEPORT_ACCEPT = new PacketType(false, "TELEPORT_ACCEPT");
            public static final PacketType TILE_NBT_QUERY = new PacketType(false, "TILE_NBT_QUERY");
            public static final PacketType SELECT_BUNDLE_ITEM = new PacketType(false, "SELECT_BUNDLE_ITEM");
            public static final PacketType DIFFICULTY_CHANGE = new PacketType(false, "DIFFICULTY_CHANGE");
            public static final PacketType CHANGE_GAME_MODE = new PacketType(false, "CHANGE_GAME_MODE");
            public static final PacketType CHAT_ACK = new PacketType(false, "CHAT_ACK");
            public static final PacketType CHAT_COMMAND_SIGNED = new PacketType(false, "CHAT_COMMAND_SIGNED");
            public static final PacketType CHAT_SESSION_UPDATE = new PacketType(false, "CHAT_SESSION_UPDATE");
            public static final PacketType CHUNK_BATCH_RECEIVED = new PacketType(false, "CHUNK_BATCH_RECEIVED");
            public static final PacketType CLIENT_COMMAND = new PacketType(false, "CLIENT_COMMAND");
            public static final PacketType CLIENT_TICK_END = new PacketType(false, "CLIENT_TICK_END");
            public static final PacketType CONFIGURATION_ACK = new PacketType(false, "CONFIGURATION_ACK");
            public static final PacketType ENCHANT_ITEM = new PacketType(false, "ENCHANT_ITEM");
            public static final PacketType CONTAINER_SLOT_STATE_CHANGED = new PacketType(false, "CONTAINER_SLOT_STATE_CHANGED");
            public static final PacketType COOKIE_RESPONSE = new PacketType(false, "COOKIE_RESPONSE");
            public static final PacketType DEBUG_SAMPLE_SUBSCRIPTION = new PacketType(false, "DEBUG_SAMPLE_SUBSCRIPTION");
            public static final PacketType B_EDIT = new PacketType(false, "B_EDIT");
            public static final PacketType ENTITY_NBT_QUERY = new PacketType(false, "ENTITY_NBT_QUERY");
            public static final PacketType JIGSAW_GENERATE = new PacketType(false, "JIGSAW_GENERATE");
            public static final PacketType DIFFICULTY_LOCK = new PacketType(false, "DIFFICULTY_LOCK");
            public static final PacketType GROUND = new PacketType(false, "GROUND");
            public static final PacketType VEHICLE_MOVE = new PacketType(false, "VEHICLE_MOVE");
            public static final PacketType BOAT_MOVE = new PacketType(false, "BOAT_MOVE");
            public static final PacketType PICK_ITEM_FROM_BLOCK = new PacketType(false, "PICK_ITEM_FROM_BLOCK");
            public static final PacketType PICK_ITEM = new PacketType(false, "PICK_ITEM");
            public static final PacketType PING_REQUEST = new PacketType(false, "PING_REQUEST");
            public static final PacketType AUTO_RECIPE = new PacketType(false, "AUTO_RECIPE");
            public static final PacketType ABILITIES = new PacketType(false, "ABILITIES");
            public static final PacketType PLAYER_LOADED = new PacketType(false, "PLAYER_LOADED");
            public static final PacketType RECIPE_SETTINGS = new PacketType(false, "RECIPE_SETTINGS");
            public static final PacketType RECIPE_DISPLAYED = new PacketType(false, "RECIPE_DISPLAYED");
            public static final PacketType ITEM_NAME = new PacketType(false, "ITEM_NAME");
            public static final PacketType RESOURCE_PACK_STATUS = new PacketType(false, "RESOURCE_PACK_STATUS");
            public static final PacketType ADVANCEMENTS = new PacketType(false, "ADVANCEMENTS");
            public static final PacketType TR_SEL = new PacketType(false, "TR_SEL");
            public static final PacketType BEACON = new PacketType(false, "BEACON");
            public static final PacketType SET_COMMAND_BLOCK = new PacketType(false, "SET_COMMAND_BLOCK");
            public static final PacketType SET_COMMAND_MINECART = new PacketType(false, "SET_COMMAND_MINECART");
            public static final PacketType SET_JIGSAW = new PacketType(false, "SET_JIGSAW");
            public static final PacketType STRUCT = new PacketType(false, "STRUCT");
            public static final PacketType SET_TEST_BLOCK = new PacketType(false, "SET_TEST_BLOCK");
            public static final PacketType SPECTATE = new PacketType(false, "SPECTATE");
            public static final PacketType TEST_INSTANCE_BLOCK_ACTION = new PacketType(false, "TEST_INSTANCE_BLOCK_ACTION");
            public static final PacketType CUSTOM_CLICK_ACTION = new PacketType(false, "CUSTOM_CLICK_ACTION");
            public static final PacketType TRANSACTION = new PacketType(false, "TRANSACTION");
            public static final PacketType CHAT_PREVIEW = new PacketType(false, "CHAT_PREVIEW");
            public static Client getInstance() { return new Client(); }
        }
    }

    public static class Login {
        public static class Server {
            public static final PacketType SUCCESS = new PacketType(true, "LOGIN_SUCCESS");
            public static final PacketType DISCONNECT = new PacketType(true, "LOGIN_DISCONNECT");
            public static final PacketType ENCRYPTION_BEGIN = new PacketType(true, "LOGIN_ENCRYPTION_BEGIN");
            public static final PacketType SET_COMPRESSION = new PacketType(true, "LOGIN_SET_COMPRESSION");
            public static final PacketType CUSTOM_PAYLOAD = new PacketType(true, "LOGIN_CUSTOM_PAYLOAD");
            public static final PacketType COOKIE_REQUEST = new PacketType(true, "LOGIN_COOKIE_REQUEST");
        }
        public static class Client {
            public static final PacketType START = new PacketType(false, "LOGIN_START");
            public static final PacketType ENCRYPTION_BEGIN = new PacketType(false, "LOGIN_ENCRYPTION_BEGIN");
            public static final PacketType CUSTOM_PAYLOAD = new PacketType(false, "LOGIN_CUSTOM_PAYLOAD");
            public static final PacketType LOGIN_ACK = new PacketType(false, "LOGIN_LOGIN_ACK");
            public static final PacketType COOKIE_RESPONSE = new PacketType(false, "LOGIN_COOKIE_RESPONSE");
        }
    }

    public static class Status {
        public static class Server {
            public static final PacketType SERVER_INFO = new PacketType(true, "SERVER_INFO");
            public static final PacketType PONG = new PacketType(true, "STATUS_PONG");
            public static final PacketType OUT_SERVER_INFO = new PacketType(true, "STATUS_OUT_SERVER_INFO");
        }
        public static class Client {
            public static final PacketType START = new PacketType(false, "STATUS_START");
            public static final PacketType PING = new PacketType(false, "STATUS_PING");
        }
    }
    public static class Handshake {
        public static class Server {
        }
        public static class Client {
            public static final PacketType SET_PROTOCOL = new PacketType(false, "HANDSHAKE_SET_PROTOCOL");
        }
    }

    public static class Configuration {
        public static class Server {
            public static final PacketType COOKIE_REQUEST = new PacketType(true, "CONFIG_COOKIE_REQUEST");
            public static final PacketType CUSTOM_PAYLOAD = new PacketType(true, "CONFIG_CUSTOM_PAYLOAD");
            public static final PacketType DISCONNECT = new PacketType(true, "CONFIG_DISCONNECT");
            public static final PacketType FINISH_CONFIGURATION = new PacketType(true, "CONFIG_FINISH_CONFIGURATION");
            public static final PacketType KEEP_ALIVE = new PacketType(true, "CONFIG_KEEP_ALIVE");
            public static final PacketType PING = new PacketType(true, "CONFIG_PING");
            public static final PacketType RESET_CHAT = new PacketType(true, "CONFIG_RESET_CHAT");
            public static final PacketType REGISTRY_DATA = new PacketType(true, "CONFIG_REGISTRY_DATA");
            public static final PacketType REMOVE_RESOURCE_PACK = new PacketType(true, "CONFIG_REMOVE_RESOURCE_PACK");
            public static final PacketType ADD_RESOURCE_PACK = new PacketType(true, "CONFIG_ADD_RESOURCE_PACK");
            public static final PacketType STORE_COOKIE = new PacketType(true, "CONFIG_STORE_COOKIE");
            public static final PacketType TRANSFER = new PacketType(true, "CONFIG_TRANSFER");
            public static final PacketType UPDATE_ENABLED_FEATURES = new PacketType(true, "CONFIG_UPDATE_ENABLED_FEATURES");
            public static final PacketType UPDATE_TAGS = new PacketType(true, "CONFIG_UPDATE_TAGS");
            public static final PacketType SELECT_KNOWN_PACKS = new PacketType(true, "CONFIG_SELECT_KNOWN_PACKS");
            public static final PacketType REPORT_DETAILS = new PacketType(true, "CONFIG_REPORT_DETAILS");
            public static final PacketType SERVER_LINKS = new PacketType(true, "CONFIG_SERVER_LINKS");
            public static final PacketType CLEAR_DIALOG = new PacketType(true, "CONFIG_CLEAR_DIALOG");
            public static final PacketType SHOW_DIALOG = new PacketType(true, "CONFIG_SHOW_DIALOG");
            public static final PacketType RESOURCE_PACK = new PacketType(true, "CONFIG_RESOURCE_PACK");
        }
        public static class Client {
            public static final PacketType CLIENT_INFORMATION = new PacketType(false, "CONFIG_CLIENT_INFORMATION");
            public static final PacketType COOKIE_RESPONSE = new PacketType(false, "CONFIG_COOKIE_RESPONSE");
            public static final PacketType CUSTOM_PAYLOAD = new PacketType(false, "CONFIG_CUSTOM_PAYLOAD");
            public static final PacketType FINISH_CONFIGURATION = new PacketType(false, "CONFIG_FINISH_CONFIGURATION");
            public static final PacketType KEEP_ALIVE = new PacketType(false, "CONFIG_KEEP_ALIVE");
            public static final PacketType PONG = new PacketType(false, "CONFIG_PONG");
            public static final PacketType RESOURCE_PACK_ACK = new PacketType(false, "CONFIG_RESOURCE_PACK_ACK");
            public static final PacketType SELECT_KNOWN_PACKS = new PacketType(false, "CONFIG_SELECT_KNOWN_PACKS");
            public static final PacketType CUSTOM_CLICK_ACTION = new PacketType(false, "CONFIG_CUSTOM_CLICK_ACTION");
        }
    }
}
