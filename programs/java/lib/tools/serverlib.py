#!/usr/bin/env python3
"""lib/server: the Java server's own classes for the Java layer (net.minecraft, CraftBukkit, Paper's internals) and the
libraries only the server uses, which plugins reach for NMS and Paper's legacy rewriter.

They are made on the computer that runs the server, from Paper's own jar: Paperclip patches Mojang's server jar
there, as it does for every Paper server. They are never shipped.

    python serverlib.py LIB [--java JAVA] [--work DIR]

LIB is a java-layer/lib folder (the layer's jar and its libraries, java/out/lib). JAVA runs Paperclip and the item
patch (Java 25 or newer; default: `java` on the PATH). DIR keeps the downloaded Paper jar and what Paperclip makes, so
a second server costs nothing (default: LIB/../paper-cache). Python 3.9+, no packages needed.

build.py uses the same steps on a developer's computer.
"""
import hashlib
import pathlib
import shutil
import subprocess
import sys
import urllib.request
import zipfile

# The Paper the layer is built against (its API jar is the layer's): 26.3 build 235.
PAPER_VERSION = '26.3'
PAPER_BUILD = 235
PAPER_SHA256 = '4ebaf5c9da98c654bb8f17245799943dfb05a285830be4307bfdf7bc9ca024bf'
PAPER_URL = f'https://fill-data.papermc.io/v1/objects/{PAPER_SHA256}/paper-{PAPER_VERSION}-{PAPER_BUILD}.jar'

# The server's libraries the layer has no use for (a console, metrics, other platforms' natives), or has its own of.
SKIP_SERVER_LIBS = ('jline', 'terminalconsoleappender', 'velocity-native', 'spark-', 'org.eclipse.sisu', 'disruptor', 'oshi-', 'msal4j', 'azure-', 'log4j-slf4j2',
                    'log4j-iostreams', 'paper-api', 'netty-transport-classes', 'netty-transport-native-epoll',
                    'netty-transport-native-io_uring', 'netty-transport-native-kqueue')


def say(*words):
    print('[serverlib]', *words, flush=True)


def paper_jar(work):
    """Paper's server jar (Paperclip), downloaded once and checked against its published checksum."""
    jar = work / f'paper-{PAPER_VERSION}-{PAPER_BUILD}.jar'
    if jar.is_file() and hashlib.sha256(jar.read_bytes()).hexdigest() == PAPER_SHA256:
        return jar
    work.mkdir(parents=True, exist_ok=True)
    say('downloading Paper', PAPER_VERSION, 'build', PAPER_BUILD)
    request = urllib.request.Request(PAPER_URL, headers={'User-Agent': 'horizonsx-kit'})
    with urllib.request.urlopen(request, timeout=300) as answer:
        data = answer.read()
    if hashlib.sha256(data).hexdigest() != PAPER_SHA256:
        sys.exit('serverlib: the Paper jar does not match its checksum; try again')
    jar.write_bytes(data)
    return jar


def patched(java, work):
    """Paperclip in patch-only mode: Mojang's server jar fetched and patched into versions/, its libraries into libraries/."""
    server_jar = work / 'versions' / PAPER_VERSION / f'paper-{PAPER_VERSION}.jar'
    libraries = work / 'libraries'
    if server_jar.is_file() and libraries.is_dir():
        return server_jar, libraries
    jar = paper_jar(work)
    say('patching Paper (Paperclip downloads the Minecraft server from Mojang)')
    done = subprocess.run([str(java), '-Dpaperclip.patchonly=true', '-jar', str(jar)], cwd=work)
    if done.returncode != 0 or not server_jar.is_file():
        sys.exit(f'serverlib: Paperclip did not make {server_jar}')
    return server_jar, libraries


def server_classes(server_jar, libraries, lib, have_names=None, have_artifacts=None):
    """lib/server: the patched server jar (signatures and services left out) and the server's own libraries that lib
    has no copy of."""
    out = lib / 'server'
    out.mkdir(parents=True, exist_ok=True)
    target = out / '0paper-server.jar'
    with zipfile.ZipFile(server_jar) as src, zipfile.ZipFile(target, 'w', zipfile.ZIP_DEFLATED) as dst:
        for info in src.infolist():
            n = info.filename
            if n.startswith('META-INF/services/') or n.endswith(('.SF', '.RSA', '.DSA')) or n.startswith('META-INF/native-image/'):
                continue
            if n == 'META-INF/MANIFEST.MF':
                dst.writestr(n, 'Manifest-Version: 1.0\r\n\r\n')
                continue
            dst.writestr(info, src.read(n))
    if have_names is None:
        have_names = {j.name for j in lib.glob('*.jar')}
    count = 0
    for jar in sorted(pathlib.Path(libraries).rglob('*.jar')):
        artifact = jar.parent.parent.name
        known = artifact in have_artifacts if have_artifacts is not None else any(n.startswith(artifact + '-') for n in have_names)
        if jar.name in have_names or known or jar.name.startswith(SKIP_SERVER_LIBS):
            continue
        shutil.copy2(jar, out / jar.name)
        count += 1
    size = sum(f.stat().st_size for f in out.iterdir()) // (1024 * 1024)
    say(f'{target.name} + {count} libraries in {out} ({size} MB)')


def drop_shadowed(lib):
    """The server jar comes first on the class path (Paper's patched Brigadier, DataFixerUpper, authlib and logging
    must win over the plain libraries), so the CraftBukkit classes the layer defines itself are taken out of it; so
    are the Paper API classes the layer stands in for, out of the API jar."""
    layer_jar = lib / 'horizon-javalayer.jar'
    server = lib / 'server' / '0paper-server.jar'
    with zipfile.ZipFile(layer_jar) as z:
        names = z.namelist()
    # CraftBukkit classes, and Moonrise's TickThread (the engine's thread counts as Paper's tick thread).
    mine = {n for n in names if n.startswith(('org/bukkit/craftbukkit/', 'ca/spottedleaf/')) and n.endswith('.class')}
    if server.is_file():
        say(f'{strip(server, mine)} classes the layer defines itself taken out of {server.name}')
    # org.bukkit.plugin.java.PluginClassLoader (libraries look for it by name on a plugin's loader) and
    # org.bukkit.event.HandlerList (its unregisters reach the layer's listeners).
    api_mine = {n for n in names if (n.startswith('org/bukkit/plugin/java/PluginClassLoader') or n == 'org/bukkit/event/HandlerList.class') and n.endswith('.class')}
    for api in lib.glob('paper-api-*.jar'):
        dropped = strip(api, api_mine)
        if dropped:
            say(f'{dropped} API classes the layer stands in for taken out of {api.name}')


def strip(jar, names):
    with zipfile.ZipFile(jar) as src:
        if not any(n in names for n in src.namelist()):
            return 0
        tmp = jar.with_suffix('.tmp')
        dropped = 0
        with zipfile.ZipFile(tmp, 'w', zipfile.ZIP_DEFLATED) as dst:
            for info in src.infolist():
                if info.filename in names:
                    dropped += 1
                    continue
                dst.writestr(info, src.read(info.filename))
    tmp.replace(jar)
    return dropped


def patch_items(java, lib, tools):
    """Paper's CraftItemStack keeps its class (libraries look its NMS statics up) but takes the layer's items too:
    its asNMSCopy/asBukkitCopy go through horizon.javalayer.nms.NmsItems for them (java/gen/PatchServer.java)."""
    asm = sorted((lib / 'server').glob('asm-9*.jar'))
    server = lib / 'server' / '0paper-server.jar'
    if not asm or not server.is_file() or not (pathlib.Path(tools) / 'PatchServer.class').is_file():
        say('the item patch was left out (no ASM, server jar or PatchServer)')
        return
    sep = ';' if sys.platform == 'win32' else ':'
    done = subprocess.run([str(java), '-cp', sep.join([str(asm[0]), str(tools)]), 'PatchServer', str(server)])
    if done.returncode != 0:
        sys.exit('serverlib: PatchServer failed')


def make(lib, java='java', work=None):
    """Everything above, for a lib folder that has the layer's jar and its libraries but no lib/server yet."""
    lib = pathlib.Path(lib).resolve()
    work = pathlib.Path(work).resolve() if work else lib.parent / 'paper-cache'
    server_jar, libraries = patched(java, work)
    if (lib / 'server').exists():
        shutil.rmtree(lib / 'server')
    server_classes(server_jar, libraries, lib)
    drop_shadowed(lib)
    patch_items(java, lib, lib / 'tools')
    say('lib/server is ready')


if __name__ == '__main__':
    args = sys.argv[1:]
    if not args or args[0] in ('-h', '--help'):
        sys.exit(__doc__)
    def opt(name, default=None):
        return args[args.index(name) + 1] if name in args else default
    make(args[0], opt('--java', 'java'), opt('--work'))
