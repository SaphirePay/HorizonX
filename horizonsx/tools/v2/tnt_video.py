#!/usr/bin/env python3
"""A video of an 8,000 TNT cube going off, as a player sees it, on server build NAME: a real Horizon client offscreen at
2560x1440 flying beside the cube (noon, in the air), frames taken through one control session for
SECONDS and joined by ffmpeg at the rate they came.

    python -I tools/v2/tnt_video.py NAME [--seconds 22] [--out D:/Horizon/perf/tnt-video] [--client-bin DIR]
"""
import os
import pathlib
import shutil
import subprocess
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import lab  # noqa: E402
import ramp  # noqa: E402
from lan_e2e import Game  # noqa: E402

PERF = ramp.PERF
option = ramp.option
CUBE = (100, 165, -12)


def main():
    name = sys.argv[1]
    seconds = float(option('--seconds', '22'))
    out = pathlib.Path(option('--out', str(PERF / 'tnt-video')))
    client_bin = pathlib.Path(option('--client-bin', str(PERF / 'hx-client-snap')))
    frames = out / f'{name}-frames'
    shutil.rmtree(frames, ignore_errors=True)
    frames.mkdir(parents=True)
    os.environ.update(lab.game_env())
    s = ramp.Server(PERF / name, PERF / '.ramp' / f'video-{name}', ramp.free_port(), ramp.RESOURCES)
    s.start(None)
    game = Game(f'video-{name}', client_bin / 'horizon-ctl.exe', lab.RESOURCES.resolve())
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'HORIZON_LAUNCHER_HOME'):
        game.d.env[key] = str(game.work / key.lower())
    try:
        game('launch', wait=False, offscreen=True, width=2560, height=1440)
        game.raw('connect', f'127.0.0.1:{s.port}', 'Viewer')
        game.raw('wait-for', 'loaded', '--timeout', 300)
        game.raw('options-set', 'enableVsync', 'false')
        game.raw('options-set', 'maxFps', '260')
        x, y, z = CUBE
        s.run('cmd', 'time', 'set', 'noon', timeout=60)
        s.run('cmd', 'gamerule', 'doDaylightCycle', 'false', timeout=60)
        s.run('cmd', 'gamemode', 'creative', 'Viewer', timeout=60)
        s.run('cmd', 'tp', 'Viewer', x - 14, y + 12, z - 14, timeout=60)
        # Creative flight where the tp put it (a platform would be blown away, dropping the player into the sea):
        # the control's fly holds the player on a short line, then leaves it flying there.
        game.raw('fly', x - 14, y + 12, z - 14, x - 14, y + 12, z - 14.05, 1)
        game.wait(120)
        game.raw('look-at', x + 10, y + 8, z + 10)
        s.run('cmd', 'fill', x, y, z, x + 19, y + 9, z + 19, 'minecraft:tnt', timeout=60)
        s.run('cmd', 'fill', x, y + 10, z, x + 19, y + 19, z + 19, 'minecraft:tnt', timeout=60)
        game.wait(60)
        # The frames: as many as one control session takes in SECONDS (a line per frame, numbered files).
        # Slow motion while recording (vanilla's /tick rate; the client follows the server's rate), so the frames a
        # 1440p capture manages make smooth real-time video once played at the game's own pace.
        rate = float(option('--rate', '5'))
        real = seconds * 20 / rate
        lines = [f'screenshot {(frames / f"f{i:05d}.png").as_posix()}' for i in range(int(real * 30))]
        took = {}

        def shoot():
            # One ctl process for every frame (no process start between frames); stops at SECONDS.
            started = time.time()
            proc = subprocess.Popen(game.d.base + ['script'], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                                    stderr=subprocess.DEVNULL, cwd=game.work, env=game.d.env, text=True)
            for line in lines:
                proc.stdin.write(line + chr(10))
            proc.stdin.flush()
            while time.time() - started < real and proc.poll() is None:
                time.sleep(0.2)
            proc.kill()
            took['s'] = time.time() - started

        shooter = threading.Thread(target=shoot)
        shooter.start()
        s.run('cmd', 'tick', 'rate', rate, timeout=60)
        time.sleep(1.5 * 20 / rate)
        s.run('cmd', 'summon', 'minecraft:tnt', x + 10, y + 10, z + 10, '{fuse:1}', timeout=60)
        shooter.join(timeout=real + 120)
        s.run('cmd', 'tick', 'rate', 20, timeout=60)
    finally:
        game.close()
        s.stop()
    shots = sorted(frames.glob('f*.png'))
    # Played back at the game's own pace: the frames span `took` real seconds = took * rate / 20 game seconds.
    fps = max(1.0, len(shots) / max(took.get('s', real) * rate / 20, 1.0))
    video = out / f'tnt-{name}.mp4'
    subprocess.run(['ffmpeg', '-y', '-loglevel', 'error', '-framerate', f'{fps:.2f}', '-i', str(frames / 'f%05d.png'),
                    '-c:v', 'libx264', '-crf', '20', '-preset', 'medium', '-pix_fmt', 'yuv420p', '-r', '30', str(video)], check=True)
    print(f'{video}: {len(shots)} frames in {took.get("s", 0):.1f} s ({fps:.1f} captured a second)')


if __name__ == '__main__':
    main()
