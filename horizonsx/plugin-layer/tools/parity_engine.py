#!/usr/bin/env python3
"""The engine the parity tests run on: the Paper engine, quick build, at out/parity/horizon-client.exe.

    parity_engine.py
"""
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
COMMIT = 'cbfae59314dfa74d54cb8a379cae609c6f5ec91d'
ENGINE = ROOT / 'out' / 'parity' / 'horizon-client.exe'

ENGINE.parent.mkdir(parents=True, exist_ok=True)
bundle = ROOT / 'bundle' / 'paper_api'
sys.exit(subprocess.run([sys.executable, '-I', str(ROOT / 'tools' / 'build_engine.py'), '--quick', COMMIT, str(ENGINE), str(bundle)], cwd=ROOT).returncode)
