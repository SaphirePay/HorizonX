"""Live check of the Rust TAB on the test server (hwe_live.py up): each command and sub answers.

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
lines = v.say('/tab help')
v.check('/tab help', has(lines, 'tab'), lines)
lines = v.say('/tab reload')
v.check('/tab reload', bool(lines) and not has(lines, 'not written yet') and not has(lines, 'Unknown'), lines)
lines = v.say('/tab download')
v.check('/tab download', bool(lines) and not has(lines, 'not written yet') and not has(lines, 'Unknown'), lines)
lines = v.say('/tab me')
v.check('/tab me', bool(lines) and not has(lines, 'not written yet') and not has(lines, 'Unknown'), lines)
lines = v.say('/tab upload')
v.check('/tab upload', bool(lines) and not has(lines, 'not written yet') and not has(lines, 'Unknown'), lines)
print(f'{v.PASSED}/{v.PASSED + v.FAILED} checks passed' if hasattr(v, 'PASSED') else 'done')
