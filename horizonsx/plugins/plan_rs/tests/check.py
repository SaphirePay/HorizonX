"""Live check of the Rust Plan on the test server (hwe_live.py up): each command and sub answers.

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
lines = v.say('/plan')
v.check('/plan', has(lines, 'Player Analytics'), lines)
lines = v.say('/plan info')
v.check('/plan info', has(lines, 'Version'), lines)
lines = v.say('/plan server')
v.check('/plan server', has(lines, 'Server page'), lines)
lines = v.say('/plan players')
v.check('/plan players', has(lines, 'Players page'), lines)
lines = v.say('/plan player')
v.check('/plan player', has(lines, 'Player page'), lines)
lines = v.say('/plan ingame')
v.check('/plan ingame', has(lines, 'Playtime'), lines)
lines = v.say('/plan search a')
v.check('/plan search a', has(lines, 'Results'), lines)
import urllib.request, json
for u in ['/', '/api/overview', '/api/online', '/api/retention', '/api/peak_hours', '/api/players', '/api/live']:
    r = urllib.request.urlopen('http://127.0.0.1:8804' + u, timeout=5)
    v.check('GET ' + u, r.status == 200, [u])
