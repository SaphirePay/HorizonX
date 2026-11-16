package com.comphenix.protocol.events;

public enum ListenerPriority { LOWEST, LOW, NORMAL, HIGH, HIGHEST, MONITOR; public int getSlot() { return ordinal(); } }
