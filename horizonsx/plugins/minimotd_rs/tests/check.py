"""Live check of the Rust MiniMOTD on the test server (hwe_live.py up): each command and sub answers.

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
# Fill each expectation with what the Java plugin says (its messages file).
lines = v.say('/minimotd help')
v.check('/minimotd help', has(lines, 'minimotd'), lines)
lines = v.say('/minimotd reload')
v.check('/minimotd reload', bool(lines) and not has(lines, 'not written yet') and not has(lines, 'Unknown'), lines)
print(f'{v.PASSED}/{v.PASSED + v.FAILED} checks passed' if hasattr(v, 'PASSED') else 'done')
