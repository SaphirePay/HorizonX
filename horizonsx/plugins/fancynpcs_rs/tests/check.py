"""Live check of the Rust FancyNpcs on the test server (hwe_live.py up): each command and sub answers.

    python tests/check.py
"""
import pathlib, sys, time
sys.path.insert(0, r'D:\Horizon\plugin-sweep\audit')
sys.path.insert(0, r'D:\Horizon\fabric-port\tools')
import proof  # noqa: E402
proof.RUN = pathlib.Path(r'D:\Horizon\parity\hwelive')
proof.PORT = 25781
import hwe_verify as v  # noqa: E402

def has(lines, word):
    return any(word.lower() in l.lower() for l in lines)

v.stand(0, 125, 0)
lines = v.say('/npc create testnpc')
v.check('/npc create testnpc', has(lines, 'created'), lines)
lines = v.say('/npc displayname testnpc <red>Hello')
v.check('/npc displayname testnpc <red>Hello', has(lines, 'display name'), lines)
lines = v.say('/npc skin testnpc Notch')
v.check('/npc skin testnpc Notch', has(lines, 'skin'), lines)
lines = v.say('/npc action testnpc right_click add message <green>Hi')
v.check('/npc action testnpc right_click add message <green>Hi', has(lines, 'added'), lines)
lines = v.say('/npc action testnpc right_click list')
v.check('/npc action testnpc right_click list', has(lines, 'Actions'), lines)
lines = v.say('/npc turn_to_player testnpc true')
v.check('/npc turn_to_player testnpc true', has(lines, 'turning'), lines)
lines = v.say('/npc glowing testnpc red')
v.check('/npc glowing testnpc red', has(lines, 'glowing'), lines)
lines = v.say('/npc info testnpc')
v.check('/npc info testnpc', has(lines, 'Identifier'), lines)
lines = v.say('/npc list')
v.check('/npc list', has(lines, 'List'), lines)
lines = v.say('/npc nearby')
v.check('/npc nearby', has(lines, 'Nearby'), lines)
lines = v.say('/npc remove testnpc')
v.check('/npc remove testnpc', has(lines, 'removed'), lines)
lines = v.say('/fancynpcs version')
v.check('/fancynpcs version', has(lines, 'FancyNpcs'), lines)
