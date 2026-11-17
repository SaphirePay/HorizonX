#!/usr/bin/env python3
"""HorizonX for Linux: assembles the ready-to-run folder and its release zip.

Usage: python packaging/linux/make_package.py VERSION --app HorizonX --engine DIR --plugins DIR --java-so FILE
           --java-lib DIR --jre JRE.tar.gz --catalog catalog.json --out DIR

  --engine     a folder with the Linux horizon-client and horizon-proxy
  --plugins    a folder with the Linux plugins (lib*.so)
  --java-so    libhorizon_java.so
  --java-lib   the Java layer's jars (the same on every system: java/lib of the Windows package)
  --jre        Eclipse Temurin 25 JRE for Linux x64 (.tar.gz)
  --catalog    the Windows catalog.json; its X.dll names become libX.so

Out: OUT/HorizonX/ (the folder) and OUT/HorizonX-VERSION-linux-x64.zip (top folder HorizonX/, every file's Unix mode
kept, so the engine and the app stay executable) and its .sha256. The app's updater takes this zip on Linux.
"""
import argparse
import hashlib
import json
import os
import pathlib
import shutil
import stat
import sys
import tarfile
import zipfile

README = """HORIZONX {version} for Linux (64-bit)

Start
  1. Unzip this folder anywhere you like, for example ~/HorizonX.
  2. Run ./HorizonX (or double-click it in your file manager).
     The first start puts HorizonX in your applications menu.
  3. Click "Make a server" and follow the five steps.

Nothing else needs to be installed on a desktop Linux with glibc 2.35 or
newer (Ubuntu 22.04+, Debian 12+, Fedora 36+, Mint 21+, Pop!_OS 22.04+,
Arch, Manjaro, openSUSE Tumbleweed, SteamOS 3...). The
server engine (HorizonsX) and its network proxy, the built-in plugins, and
the Java layer with a Java runtime are all in this folder. HorizonX draws
with Vulkan or OpenGL, on X11 or Wayland.

The first time
  HorizonX prepares the Minecraft 26.3 server files from your own copy of
  Minecraft: Java Edition (~/.minecraft). Play 26.3 once on this PC first.
  This takes a few minutes and happens only once.

No screen (a headless server)
  The app needs libxkbcommon-x11 even without a screen (Debian/Ubuntu:
  sudo apt install libxkbcommon-x11-0; Fedora: libxkbcommon-x11).
  ./HorizonX mcp      the AI assistant server (everything the app can do)
  ./HorizonX update   update now
  ./HorizonX version  this copy's version

Friends joining from anywhere
  Every server gets a public address through playit.gg, with no port
  forwarding. The first time, HorizonX shows a link to claim the address.
  Don't want one? Turn playit off while making the server (the wizard's
  Plugins step), or later on its Plugins page.

Plugins
  28 plugins written in Rust come built in: pick them while making a server,
  or add them later on its Plugins page (EssentialsX, LuckPerms, CoreProtect,
  WorldEdit, Multiverse, mcMMO, Jobs, ChestShop, TAB and more).
  Search Modrinth on the Plugins page and click Add, or click "Add a .jar" for
  a Paper or Spigot plugin you already have. The Java layer and its own Java 25
  runtime come with HorizonX; it has its own switch on each server.

Opening at sign-in
  Settings > General > "Open HorizonX when you sign in" adds it to
  ~/.config/autostart (GNOME, KDE, Xfce, Cinnamon, MATE, LXQt...).

Updates
  HorizonX updates itself from GitHub: it checks when it opens and every 6
  hours, then downloads, checks and installs the new version on its own.
  Running servers keep running and use the new engine at their next restart.
  You don't need to download this zip again.

Folder contents
  HorizonX                  the app
  engines/horizonsx/        the HorizonsX server engine and horizon-proxy
  plugins/                  plugins HorizonX can install with one click
  java/                     the Java layer for Paper/Spigot plugins, and Java 25
                            (Eclipse Temurin, GPLv2 with Classpath Exception)
"""


def linux_name(dll):
    return 'lib' + dll[:-4] + '.so' if dll.endswith('.dll') else dll


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('version')
    for a in ('--app', '--engine', '--plugins', '--java-so', '--java-lib', '--jre', '--catalog', '--out'):
        ap.add_argument(a, required=True)
    o = ap.parse_args()
    out = pathlib.Path(o.out)
    root = out / 'HorizonX'
    shutil.rmtree(root, ignore_errors=True)
    (root / 'engines' / 'horizonsx').mkdir(parents=True)
    (root / 'plugins').mkdir()
    (root / 'java').mkdir()
    modes = {}  # path relative to root -> mode

    def put(src, rel, mode):
        dst = root / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, dst)
        modes[rel] = mode

    put(o.app, 'HorizonX', 0o755)
    for prog in ('horizon-client', 'horizon-proxy'):
        put(pathlib.Path(o.engine) / prog, f'engines/horizonsx/{prog}', 0o755)
    catalog = json.loads(pathlib.Path(o.catalog).read_text(encoding='utf-8'))
    plugins = pathlib.Path(o.plugins)
    missing = []
    for entry in catalog:
        entry['file'] = linux_name(entry['file'])
        src = plugins / entry['file']
        if src.is_file():
            put(src, f"plugins/{entry['file']}", 0o755)
        else:
            missing.append(entry['file'])
    if missing:
        sys.exit('plugins in the catalog with no Linux build: ' + ', '.join(missing))
    (root / 'plugins' / 'catalog.json').write_text(
        '[\n' + ',\n'.join('  ' + json.dumps(e, ensure_ascii=False) for e in catalog) + '\n]\n', encoding='utf-8')
    modes['plugins/catalog.json'] = 0o644
    put(o.java_so, 'java/libhorizon_java.so', 0o755)
    for f in sorted(pathlib.Path(o.java_lib).rglob('*')):
        if f.is_file():
            put(f, 'java/lib/' + f.relative_to(o.java_lib).as_posix(), 0o644)
    # The JRE keeps its own modes (bin/java, lib/jspawnhelper...); its top folder (jdk-25...-jre) becomes runtime/.
    with tarfile.open(o.jre) as t:
        for m in t.getmembers():
            parts = pathlib.PurePosixPath(m.name).parts
            if len(parts) < 2 or not (m.isfile() or m.issym()):
                continue
            rel = 'java/runtime/' + '/'.join(parts[1:])
            dst = root / rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            if m.issym():
                # A link in the JRE (rare): its target's bytes, so the zip stays plain files.
                continue
            with t.extractfile(m) as src, open(dst, 'wb') as d:
                shutil.copyfileobj(src, d)
            modes[rel] = m.mode & 0o777 or 0o644
    (root / 'READ ME FIRST.txt').write_text(README.format(version=o.version), encoding='utf-8', newline='\n')
    modes['READ ME FIRST.txt'] = 0o644

    zip_path = out / f'HorizonX-{o.version}-linux-x64.zip'
    with zipfile.ZipFile(zip_path, 'w', zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for rel in sorted(modes):
            info = zipfile.ZipInfo('HorizonX/' + rel, date_time=(2026, 1, 1, 0, 0, 0))
            info.create_system = 3  # Unix: the mode below is read
            info.external_attr = (stat.S_IFREG | modes[rel]) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            with open(root / rel, 'rb') as f:
                z.writestr(info, f.read())
    digest = hashlib.sha256(zip_path.read_bytes()).hexdigest()
    (out / (zip_path.name + '.sha256')).write_text(f'{digest}  {zip_path.name}\n', encoding='utf-8')
    size = sum((root / r).stat().st_size for r in modes)
    print(f'{zip_path} {zip_path.stat().st_size / 1e6:.0f} MB zipped, {size / 1e6:.0f} MB unpacked, {len(modes)} files')
    print(f'sha256 {digest}')


if __name__ == '__main__':
    main()
