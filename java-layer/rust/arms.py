"""The binding arms the Java layer has for a method name (src/generated.rs): which class/receiver, how many
arguments, the argument checks, and the SDK call. For when a call reaches the server but no arm matches.

    python arms.py selectEntities [registerNewObjective ...]
"""
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
TEXT = (HERE / 'src' / 'generated.rs').read_text(encoding='utf-8')


def receivers():
    """fn nc_N -> the Rust type it is for."""
    out = {}
    for m in re.finditer(r'fn (nc_\d+)\(r: &V, name: &str, a: &\[V\]\) -> Option<R> \{ let (?:held_|ev_) = r\.native::<(?:Mutex<|jbridge::EvL<)(.*?)>>\(\)\?', TEXT):
        out[m.start()] = (m.group(1), m.group(2))
    return sorted(out.items())


def owner_at(pos, recv):
    best = None
    for start, info in recv:
        if start <= pos:
            best = info
        else:
            break
    return best


def main(names):
    recv = receivers()
    for name in names:
        print(f'== {name}')
        for m in re.finditer(r'if (class == "(\w+)" && )?name == "' + re.escape(name) + r'" && a\.len\(\) == (\d+)(.*?)\{(.*?)\}', TEXT):
            cls, n, cond, body = m.group(2), m.group(3), m.group(4), m.group(5)
            where = f'static {cls}' if cls else f'on {owner_at(m.start(), recv)[1] if owner_at(m.start(), recv) else "?"}'
            call = re.search(r'(horizon_bukkit::[\w:]+|t\.\w+)\(', body)
            cond = re.sub(r'\s+', ' ', cond).strip(' &')
            print(f'  {where:55} args={n}  {cond[:140]}  -> {call.group(1) if call else "?"}')


if __name__ == '__main__':
    main(sys.argv[1:])
