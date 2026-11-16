"""Java layer tools for the plugin-port MCP server: unchanged Bukkit/Paper jars on HorizonsX.

jl_build (layer build), jl_scan (static jar check), jl_sweep (corpus run in the background), jl_report / jl_why
(results), jl_try (a live server + client: jars in, commands out), jl_log.
Sources: <workspace>/paper-engine/java-layer (test/sweep.py, test/jlscan.py, test/jl.py).
"""
import workspace  # noqa: E402
import json
import os
import pathlib
import subprocess
import sys

LAYER = workspace.at('paper-engine', 'java-layer')
TEST = LAYER / 'test'
PY = sys.executable
SWEEPS = workspace.at('parity', 'jlsweep')
VENV = str(workspace.at('plugin-tools', 'jport', '.venv', 'Scripts', 'python.exe'))


def _run(args, cwd=TEST, timeout=900, env=None):
    done = subprocess.run(args, cwd=str(cwd), capture_output=True, text=True, encoding='utf-8', errors='replace',
                          stdin=subprocess.DEVNULL, timeout=timeout, env=env or dict(os.environ, PYTHONIOENCODING='utf-8'))
    return done.returncode, (done.stdout + done.stderr)


def _tail(text, n=40):
    lines = [l for l in text.splitlines() if l.strip()]
    return '\n'.join(lines[-n:])


def jl_build(java=True, rust=True, gen=False):
    out = []
    if java:
        code, text = _run([PY, 'build.py'], cwd=LAYER / 'java')
        errors = [l for l in text.splitlines() if 'error:' in l][:20]
        out.append(f'java: {"OK" if code == 0 else "FAILED"} ' + ('\n' + '\n'.join(errors) if errors else _tail(text, 2)))
        if code != 0 and not errors:
            out.append(_tail(text, 30))
    if gen:
        code, text = _run([VENV, 'gen.py', '--check'], cwd=LAYER / 'rust', timeout=1800)
        out.append(f'gen: {"OK" if code == 0 else "FAILED"} ' + _tail(text, 3))
    if rust:
        code, text = _run(['cargo', '+1.99.1', 'build', '--release'], cwd=LAYER / 'rust', timeout=1800)
        errs = [l for l in text.splitlines() if l.startswith('error') or l.lstrip().startswith('-->')][:30]
        out.append(f'rust: {"OK" if code == 0 else "FAILED"} ' + ('\n'.join(errs) if errs else _tail(text, 1)))
    return '\n'.join(out)


def jl_scan(jar='', top=False):
    if top or not jar:
        return _run([PY, 'jlscan.py', 'top', '30'])[1]
    path = pathlib.Path(jar)
    if not path.is_file():
        rows = json.loads((TEST / 'corpus.json').read_text(encoding='utf-8'))['jars']
        hit = next((r for r in rows if (r.get('name') or '').lower() == jar.lower()), None) or next((r for r in rows if jar.lower() in r['file'].lower()), None)
        if not hit:
            return f'no jar or corpus plugin named {jar}'
        return json.dumps(hit, indent=1)
    return _run([PY, 'jlscan.py', str(path)])[1]


def jl_sweep(run, only='', group=20, lanes=2, failed='', paper_ok=False, settle=6):
    """Starts in the background; the reply is immediate. Progress and results: jl_report."""
    args = [PY, 'sweep.py', 'run', run, '--group', str(group), '--lanes', str(lanes), '--settle', str(settle)]
    if only:
        args += ['--only', only]
    if failed:
        args += ['--failed', failed]
    if paper_ok:
        args.append('--paper-ok')
    (SWEEPS / run).mkdir(parents=True, exist_ok=True)
    log = open(SWEEPS / run / 'sweep.log', 'a', encoding='utf-8')
    flags = (subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP) if os.name == 'nt' else 0
    p = subprocess.Popen(args, cwd=str(TEST), stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT, creationflags=flags,
                         env=dict(os.environ, PYTHONIOENCODING='utf-8', PYTHONUNBUFFERED='1'))
    return f'sweep {run} started (pid {p.pid}); log {SWEEPS / run / "sweep.log"}; check with jl_report run={run}'


def jl_report(run, vs=''):
    folder = SWEEPS / run
    if not (folder / 'results.json').is_file():
        log = folder / 'sweep.log'
        return 'no results yet. ' + (_tail(log.read_text(encoding='utf-8', errors='replace'), 10) if log.is_file() else '')
    args = [PY, 'sweep.py', 'report', run] + (['--vs', vs] if vs else [])
    text = _run(args)[1]
    log = folder / 'sweep.log'
    progress = _tail(log.read_text(encoding='utf-8', errors='replace'), 3) if log.is_file() else ''
    return progress + '\n\n' + text


def jl_why(run, plugin):
    return _run([PY, 'sweep.py', 'why', run, plugin])[1]


def jl_try(jars='', commands='', restart=False):
    """jars: comma-separated paths or corpus names (server restarts with them); commands: `;`-separated, no slash."""
    out = []
    paths = []
    if jars:
        rows = json.loads((TEST / 'corpus.json').read_text(encoding='utf-8'))['jars']
        by = {(r.get('name') or '').lower(): r for r in rows}
        for j in [x.strip() for x in jars.split(',') if x.strip()]:
            if pathlib.Path(j).is_file():
                paths.append(j)
            elif j.lower() in by:
                row = by[j.lower()]
                paths.append(row['jar'])
                for d in row.get('depend', []):
                    if d.lower() in by and by[d.lower()]['jar'] not in paths:
                        paths.append(by[d.lower()]['jar'])
            else:
                return f'no jar {j}'
    if paths or restart:
        code, text = _run([PY, 'jl.py', 'reload', *paths], timeout=300)
        if 'not running' in text.lower() or code != 0:
            code, text = _run([PY, 'jl.py', 'up', *paths], timeout=300)
        out.append(_tail(text, 15))
    if commands:
        code, text = _run([PY, 'jl.py', 'cmd', *[c.strip() for c in commands.split(';') if c.strip()]], timeout=300)
        out.append(text.strip())
    return '\n'.join(out) or 'nothing to do'


def jl_log(lines=60):
    return _run([PY, 'jl.py', 'log', str(lines)])[1]


def jl_probe(snippet='', probes='', accept=False):
    """A Java snippet inside the layer (jprobe), or the probe suite (probes/*.java, one server start) against its
    baseline: what got fixed, broke or changed."""
    if snippet:
        code, text = _run([PY, 'jprobe.py', snippet], timeout=600)
        return _tail(text, 60)
    args = [PY, 'probe_all.py'] + [p for p in probes.replace(',', ' ').split() if p]
    if accept:
        args.append('--accept')
    code, text = _run(args, timeout=900)
    return _tail(text, 80)


TOOLS = {
    'jl_build': (jl_build, 'Build the Java layer (Java side -> java/out/lib; Rust DLL; gen=true also regenerates the SDK binding table first). Short OK/FAILED with the errors.', {'java': 'boolean', 'rust': 'boolean', 'gen': 'boolean'}),
    'jl_scan': (jl_scan, 'Static check of a plugin jar (path or corpus name) for the Java layer: plugin.yml, Java version, NMS/CraftBukkit links and reflection, PacketEvents, Netty, other plugins, unresolved libraries. No jar / top=true: what blocks the most corpus jars.', {'jar': 'string', 'top': 'boolean'}),
    'jl_sweep': (jl_sweep, 'Run corpus plugins on the Java layer + HorizonsX engine in the background (groups, crash-splitting). only = comma names; failed = an older run (only its failures); paper_ok = only jars known to enable on real Paper. Check with jl_report.', {'run': 'string', 'only': 'string', 'group': 'integer', 'lanes': 'integer', 'failed': 'string', 'paper_ok': 'boolean', 'settle': 'integer'}),
    'jl_report': (jl_report, 'A Java layer sweep: progress, score (works / enabled, vs Paper), failures grouped by cause, top layer gaps and missing classes; vs = an older run for gained/lost.', {'run': 'string', 'vs': 'string'}),
    'jl_why': (jl_why, "One plugin's row in a sweep: state, errors with stack frames, gaps, missing classes, static facts.", {'run': 'string', 'plugin': 'string'}),
    'jl_try': (jl_try, 'The live Java layer test server (<workspace>/parity/jl_live, a client on it as op Prover): jars = comma paths or corpus names (restarts with them + their deps), commands = `;`-separated without slash; gives chat replies and layer log lines per command.', {'jars': 'string', 'commands': 'string', 'restart': 'boolean'}),
    'jl_probe': (jl_probe, 'Java layer checks in a scratch server: snippet = Java statements (body of onEnable; t("name", () -> value) records a check); or no snippet = the whole probe suite (test/probes, probes = some names) compared with its baseline (accept=true makes this run the baseline).', {'snippet': 'string', 'probes': 'string', 'accept': 'boolean'}),
    'jl_log': (jl_log, "The live Java layer server's last layer/error log lines.", {'lines': 'integer'}),
}
