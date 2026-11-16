"""Live check of the Rust BetterRTP on the test server (hwe_live.py up): each command and sub answers.

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
lines = v.say('/rtp help')
v.check('/rtp help', has(lines, 'BetterRTP'), lines)
lines = v.say('/rtp version')
v.check('/rtp version', has(lines, 'Version'), lines)
lines = v.say('/rtp info')
v.check('/rtp info', has(lines, 'Info'), lines)
lines = v.say('/rtp info world world')
v.check('/rtp info world world', has(lines, 'Max Radius'), lines)
lines = v.say('/rtp world world')
v.check('/rtp world world', has(lines, 'Teleport'), lines)
lines = v.say('/rtp')
v.check('/rtp', has(lines, 'Whoops'), lines)
lines = v.say('/rtp reload')
v.check('/rtp reload', has(lines, 'reloaded'), lines)
lines = v.say('/rtp nonsense')
v.check('/rtp nonsense', has(lines, 'Invalid'), lines)
