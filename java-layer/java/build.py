"""Builds the Java side of the Java layer into out/lib: horizon-javalayer.jar plus the Paper API and the libraries a
Paper server gives its plugins. A server needs that folder as <server>/java-layer/lib.

python build.py [--jdk <java 25 home>] [--libs <a Paper server's libraries folder>]
"""
import argparse
import os
import pathlib
import shutil
import subprocess
import sys
import zipfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import serverlib  # noqa: E402

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE / 'out'
LIB = OUT / 'lib'

# What a Paper 26.3 server puts on its plugins' class path (the API, its libraries, the JDBC drivers plugins use).
WANTED = [
    'paper-api', 'adventure-api', 'adventure-key', 'adventure-text-minimessage', 'adventure-text-serializer-ansi',
    'adventure-text-serializer-commons', 'adventure-text-serializer-gson', 'adventure-text-serializer-json',
    'adventure-text-serializer-legacy', 'adventure-text-serializer-plain', 'adventure-text-logger-slf4j', 'option', 'ansi',
    'gson', 'guava', 'failureaccess', 'snakeyaml', 'joml', 'fastutil', 'brigadier', 'authlib', 'datafixerupper', 'bungeecord-chat', 'log4j-api', 'log4j-core',
    'slf4j-api', 'jspecify', 'commons-lang3', 'commons-lang', 'commons-io', 'commons-codec', 'json-simple',
    'error_prone_annotations', 'geantyref', 'sqlite-jdbc', 'mysql-connector-j',
    # Netty, which Paper has and plugins use for byte buffers and plugin messages (not the server's own channels).
    'netty-buffer', 'netty-common', 'netty-transport', 'netty-codec', 'netty-codec-base', 'netty-handler', 'netty-resolver',
    'netty-codec-compression', 'netty-codec-haproxy', 'netty-codec-http', 'netty-transport-native-unix-common',
    # Netty's epoll/kqueue classes, which plugins link (Geyser, PlasmoVoice: Epoll.isAvailable() then NIO without the natives).
    'netty-transport-classes-epoll', 'netty-transport-classes-kqueue',
    # Their Linux natives (x86_64 and aarch64, as Paper ships both): on Linux Epoll.isAvailable() is true, as on Paper.
    'netty-transport-native-epoll',
    # Maven Resolver, which a Paper plugin's loader (paper-plugin.yml `loader:`) uses through MavenLibraryResolver:
    # its Dependency, DefaultArtifact and RemoteRepository classes are on a plugin's class path on Paper too.
    'maven-resolver-api', 'maven-resolver-spi', 'maven-resolver-impl', 'maven-resolver-util', 'maven-resolver-named-locks',
    'maven-resolver-connector-basic', 'maven-resolver-transport-http', 'maven-resolver-provider', 'maven-model',
    'maven-model-builder', 'maven-builder-support', 'maven-artifact', 'maven-repository-metadata', 'plexus-utils',
    'plexus-interpolation', 'httpclient', 'httpcore', 'jcl-over-slf4j',
    # OSHI and its JNA, which Paper has for its own system report and plugins' metrics libraries read too (AxGraves,
    # AxTrade: oshi.SystemInfo).
    'oshi-core', 'jna', 'jna-platform',
]


# Artifacts kept in every one of their jars (one per platform), not the first found.
EVERY_JAR = ('netty-transport-native-epoll',)


def find_libs(root):
    found = {}
    for jar in pathlib.Path(root).rglob('*.jar'):
        artifact = jar.parent.parent.name
        if artifact in WANTED and artifact not in found:
            found[artifact] = jar
        elif artifact in EVERY_JAR:
            found[jar.stem] = jar
    return found


def run(command, **kw):
    print('>', ' '.join(str(c)[:60] for c in command[:4]), '...' if len(command) > 4 else '')
    done = subprocess.run([str(c) for c in command], **kw)
    if done.returncode != 0:
        sys.exit(done.returncode)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--jdk', default=os.environ.get('JDK25', r'C:\Program Files\Java\jdk-25'))
    parser.add_argument('--libs', default=r'D:\Horizon\parity\jport\paper\libraries')
    parser.add_argument('--sqlite-jar', default=None)
    args = parser.parse_args()
    jdk = pathlib.Path(args.jdk)
    javac, java, jar_tool = (jdk / 'bin' / ('javac.exe' if os.name == 'nt' else 'javac')), (jdk / 'bin' / ('java.exe' if os.name == 'nt' else 'java')), (jdk / 'bin' / ('jar.exe' if os.name == 'nt' else 'jar'))
    libs = find_libs(args.libs)
    # The Paper server jar's own copies of what the extracted folder lacks (sqlite, mysql are bundled there).
    missing = [w for w in WANTED if w not in libs]
    paper = pathlib.Path(r'D:\Horizon\parity\paper-26.3-235.jar')
    extra = OUT / 'from-paper'
    if missing and paper.is_file():
        with zipfile.ZipFile(paper) as z:
            for name in z.namelist():
                if not name.startswith('META-INF/libraries/') or not name.endswith('.jar'):
                    continue
                artifact = name.split('/')[-3]
                if artifact in missing:
                    target = extra / pathlib.Path(name).name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(z.read(name))
                    libs[artifact] = target
    still = [w for w in WANTED if w not in libs]
    if still:
        print('not found (left out):', ', '.join(still))
    if LIB.exists():
        try:
            shutil.rmtree(LIB)
        except OSError as error:
            sys.exit(f'BUILD FAILED: {LIB} is in use (a server runs from it?): {error}')
    LIB.mkdir(parents=True)
    for artifact, jar in sorted(libs.items()):
        shutil.copy2(jar, LIB / jar.name)
    sep = ';' if os.name == 'nt' else ':'
    classpath = sep.join(str(LIB / j.name) for j in libs.values())
    # The server's classes are on the compile class path (the layer's Craft* classes extend Paper's, and the NMS
    # bridge uses vanilla's types); at run time they come from lib/server, after the layer's jar.
    if SERVER_JAR.is_file():
        serverlib.server_classes(SERVER_JAR, args.libs, LIB, {j.name for j in libs.values()}, set(libs))
    else:
        print('no Paper server jar at', SERVER_JAR, '- lib/server left out (NMS and legacy plugins will not work)')
    classpath = sep.join([classpath] + [str(j) for j in sorted((LIB / 'server').glob('*.jar'))])

    gen_classes = OUT / 'gen-classes'
    gen_src = OUT / 'gen-src'
    classes = OUT / 'classes'
    for d in (gen_classes, gen_src, classes):
        if d.exists():
            shutil.rmtree(d)
        d.mkdir(parents=True)
    run([javac, '-nowarn', '-d', gen_classes, '-cp', classpath, HERE / 'gen' / 'Gen.java', HERE / 'gen' / 'PatchServer.java'])
    run([java, '-cp', classpath + sep + str(gen_classes), 'Gen', gen_src])
    sources = [str(p) for p in (HERE / 'src').rglob('*.java')] + [str(p) for p in gen_src.rglob('*.java')]
    listing = OUT / 'sources.txt'
    listing.write_text('\n'.join(f'"{s}"'.replace('\\', '/') for s in sources), encoding='utf-8')
    run([javac, '-nowarn', '-Xlint:none', '-encoding', 'UTF-8', '--release', '25', '-proc:none', '-d', classes, '-cp', classpath, f'@{listing}'])
    services = classes / 'META-INF' / 'services'
    services.mkdir(parents=True, exist_ok=True)
    rows = (gen_src / 'services.txt').read_text(encoding='utf-8').split()
    rows.append('io.papermc.paper.registry.RegistryAccess=horizon.javalayer.Registries')
    rows.append('io.papermc.paper.command.brigadier.MessageComponentSerializer=horizon.javalayer.MessageSerializer')
    # Adventure's plain and legacy serializers with Paper's flattener (the game's en_us words for translatable text).
    rows.append('net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer$Provider=horizon.javalayer.Translations$Plain')
    rows.append('net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer$Provider=horizon.javalayer.Translations$Legacy')
    # Paper's own class loader storage: plugin libraries read its fields by reflection (gestalt's globalGroup).
    rows.append('io.papermc.paper.plugin.provider.classloader.PaperClassLoaderStorage=io.papermc.paper.plugin.entrypoint.classloader.group.PaperPluginClassLoaderStorage')
    # Moonrise's hooks into Paper (the server jar's services are left out of lib/server): Paper's settings read them.
    rows.append('ca.spottedleaf.moonrise.common.PlatformHooks=ca.spottedleaf.moonrise.paper.PaperHooks')
    # SLF4J lines to the server's console (Log4j's: log4j-core, as on Paper, configured by logging.LayerLog4j).
    rows.append('org.slf4j.spi.SLF4JServiceProvider=horizon.javalayer.logging.Slf4jProvider')
    for row in rows:
        interface, impl = row.split('=')
        (services / interface).write_text(impl + '\n', encoding='utf-8')
    shutil.copy(gen_src / 'resources' / 'horizon-events.txt', classes / 'horizon-events.txt')
    # Paper 26.3's own default config files (config/paper-global.yml...), written for servers that have none.
    shutil.copytree(HERE / 'resources', classes, dirs_exist_ok=True)
    # The built-in ProtocolLib (its API over the server's packet hooks) in a jar of its own, off the class path: the
    # layer hands its classes to plugins only when the server has no real ProtocolLib jar (Builtins.standInClass).
    stand_in = OUT / 'builtin-protocollib'
    shutil.rmtree(stand_in, ignore_errors=True)
    (stand_in / 'com').mkdir(parents=True)
    shutil.move(str(classes / 'com' / 'comphenix'), str(stand_in / 'com' / 'comphenix'))
    builtin = LIB / 'builtin'
    builtin.mkdir(exist_ok=True)
    run([jar_tool, '--create', '--file', builtin / 'ProtocolLib.jar', '-C', stand_in, '.'])
    target = LIB / 'horizon-javalayer.jar'
    run([jar_tool, '--create', '--file', target, '-C', classes, '.'])
    print('built', target, f'({target.stat().st_size // 1024} KB), {len(libs)} libraries in', LIB)
    serverlib.drop_shadowed(LIB)
    serverlib.patch_items(java, LIB, gen_classes)
    # What a server's computer needs to make lib/server itself (the HorizonsX kit runs it): serverlib and PatchServer.
    tools = LIB / 'tools'
    tools.mkdir(exist_ok=True)
    shutil.copy2(HERE / 'serverlib.py', tools / 'serverlib.py')
    for cls in gen_classes.glob('PatchServer*.class'):
        shutil.copy2(cls, tools / cls.name)


# The Java server's own classes (net.minecraft, CraftBukkit, Paper's internals) and the libraries only the server
# uses: lib/server, loaded by the layer in a class loader of their own (parent-first: the layer's CraftServer and
# CraftPlayer stay in charge) and only touched when a plugin reaches for them (NMS, Paper's legacy rewriter).
# Built from a Paper server jar made on this computer (Paperclip patches Mojang's own server jar); never shipped.
SERVER_JAR = pathlib.Path(r'D:\Horizon\parity\jport\paper\versions\26.3\paper-26.3.jar')


if __name__ == '__main__':
    main()
