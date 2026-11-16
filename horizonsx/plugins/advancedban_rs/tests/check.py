"""Live check of the Rust AdvancedBan on the test server (hwe_live.py up): each command and sub answers.

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
lines = v.say('/advancedban')
v.check('/advancedban', has(lines, 'AdvancedBan'), lines)
lines = v.say('/ban')
v.check('/ban', has(lines, 'Usage'), lines)
lines = v.say('/tempmute Notch 10m spam')
v.check('/tempmute Notch 10m spam', has(lines, 'muted'), lines)
lines = v.say('/check Notch')
v.check('/check Notch', has(lines, 'Checking'), lines)
lines = v.say('/warn Notch test')
v.check('/warn Notch test', has(lines, 'warned'), lines)
lines = v.say('/warns Notch')
v.check('/warns Notch', has(lines, 'Warnings'), lines)
lines = v.say('/history Notch')
v.check('/history Notch', has(lines, 'History'), lines)
lines = v.say('/banlist')
v.check('/banlist', has(lines, 'Banlist'), lines)
lines = v.say('/unmute Notch')
v.check('/unmute Notch', has(lines, 'unmuted'), lines)
lines = v.say('/unwarn clear Notch')
v.check('/unwarn clear Notch', has(lines, 'Cleared'), lines)
