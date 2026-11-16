"""Live check of the Rust Multiverse-Core on the test server (hwe_live.py up): each command and sub answers.

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
lines = v.say('/mv')
v.check('/mv', has(lines, 'version'), lines)
lines = v.say('/mv version')
v.check('/mv version', has(lines, 'Multiverse Core'), lines)
lines = v.say('/mv list')
v.check('/mv list', has(lines, 'World List'), lines)
lines = v.say('/mv info')
v.check('/mv info', has(lines, 'World Info'), lines)
lines = v.say('/mv create mvtest normal')
v.check('/mv create mvtest normal', has(lines, 'created'), lines)
lines = v.say('/mv modify mvtest set pvp false')
v.check('/mv modify mvtest set pvp false', has(lines, 'Successfully'), lines)
lines = v.say('/mv tp mvtest')
v.check('/mv tp mvtest', has(lines, 'Teleported'), lines)
lines = v.say('/mv who')
v.check('/mv who', has(lines, 'Players'), lines)
lines = v.say('/mv gamerule set keepInventory true mvtest')
v.check('/mv gamerule set keepInventory true mvtest', has(lines, 'Successfully'), lines)
lines = v.say('/mv setspawn')
v.check('/mv setspawn', has(lines, 'spawn'), lines)
lines = v.say('/mv unload mvtest')
v.check('/mv unload mvtest', has(lines, 'unloaded'), lines)
lines = v.say('/mv load mvtest')
v.check('/mv load mvtest', has(lines, 'loaded'), lines)
lines = v.say('/mv delete mvtest')
v.check('/mv delete mvtest', has(lines, 'confirm'), lines)
lines = v.say('/mvconfirm')
v.check('/mvconfirm', has(lines, 'OTP'), lines)
v.done() if hasattr(v, 'done') else None
