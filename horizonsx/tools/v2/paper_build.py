#!/usr/bin/env python3
"""Build Horizon V2 with the Paper plugin layer: Paper plugins run on the V2 server.

Usage: python tools/v2/paper_build.py NAME [--check] [--stage-only] [--quick]

The same steps as the plugin layer's build_engine.py + dim_patch.py (plugin-layer/), but on
this tree instead of the pinned GitHub source:
  1. this tree (no .git, no target) is copied to ../build/horizonsx-paper/tree
  2. the plugin layer's bundle, paper_api, is applied with horizon-mixins
  3. dim_patch: created worlds (Dim::Custom) and every plugin-layer/family/*_engine.py row
  4. compiled (profile fast, Segment Heap) with its own cache, ../build/horizonsx-paper-target
The engine lands in ../build/engines/NAME/horizon-client.exe (horizon-client on Linux and macOS).

horizon-mixins (https://github.com/horizon-rust/horizon-mixins) must be on the PATH:
`cargo install --git https://github.com/horizon-rust/horizon-mixins`, or name it with HORIZON_MIXINS.
Folders can be moved with HX_PLUGIN_LAYER, HX_PAPER_WORK, HX_PAPER_TARGET and HX_ENGINES.
The Paper engine's files are only read, never written. Plugins go in the server's plugins/ folder. A family row whose text V2 already carries (the
speed and RAM rows V2 took by hand) is skipped; a row found neither way is listed and stops the build.
"""
import importlib.util
import os
import pathlib
import shutil
import subprocess
import sys
import time

TREE = pathlib.Path(__file__).resolve().parents[2]
BUILD = TREE.parent / 'build'
ENGINE = pathlib.Path(os.environ.get('HX_PLUGIN_LAYER', TREE / 'plugin-layer'))
FAMILY = ENGINE / 'family'
WORK = pathlib.Path(os.environ.get('HX_PAPER_WORK', BUILD / 'horizonsx-paper'))
CACHE = pathlib.Path(os.environ.get('HX_PAPER_TARGET', BUILD / 'horizonsx-paper-target'))
PERF = pathlib.Path(os.environ.get('HX_ENGINES', BUILD / 'engines'))
COMMIT = 'cbfae59314dfa74d54cb8a379cae609c6f5ec91d'
BUNDLES = ('paper_api',)
BELOW_NORMAL = 0x00004000


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# Program names: horizon-client.exe on Windows, horizon-client on Linux and macOS.
EXE = '.exe' if os.name == 'nt' else ''


def copy_tree(target):
    """The worktree as it is now (uncommitted edits too); robocopy (Windows) or rsync (Linux, macOS) keep file times,
    so cargo rebuilds little."""
    if os.name != 'nt':
        target.mkdir(parents=True, exist_ok=True)
        if shutil.which('rsync'):
            subprocess.run(['rsync', '-a', '--delete', '--exclude', '.git', '--exclude', 'target', f'{TREE}/', f'{target}/'],
                           check=True)
        else:
            shutil.rmtree(target, ignore_errors=True)
            shutil.copytree(TREE, target, ignore=shutil.ignore_patterns('.git', 'target'), symlinks=True)
        return
    result = subprocess.run(['robocopy', str(TREE), str(target), '/MIR', '/XD', '.git', 'target', '/XF', '.git',
                             '/NFL', '/NDL', '/NJH', '/NJS', '/NP'], capture_output=True, text=True)
    if result.returncode >= 8:
        sys.exit(f'robocopy failed ({result.returncode}): {result.stdout[-800:]}')


# dim_patch.loose_span, once loaded.
LOOSE = [lambda text, old: None]


# The port's speed and RAM rows: V2 carries its own version of each (docs/v2/README.md), so one
# that doesn't fit is V2's form of it, not a missing plugin feature.
V2_OWN = ('perf_engine', 'ram_engine')


# Port rows V2's own rows (paper_v2_rows.py) take the place of: the text they change is V2's.
# entity_engine#226: the freezing row's first form; V2's motion.rs has that line once, after effects_from_blocks,
# which #227 changes.
REPLACED = {'entity_engine#208', 'entity_engine#226'}

# Port rows whose text V2 changed for its own speed: the same change, written against V2's text.
V2_FORMS = {
    # HorizonsX 11-07: rows whose text V2 changed since (11-06 16:20 onwards): the same change on today's text.
    # Spawning walks each player chunk once (34e43cef9): once for each chunk and plugin reach, so a player whose
    # reach a plugin changed (PlayerNaturallySpawnCreaturesEvent) keeps it.
    'ev_entity_engine#21': (
        '''        let mut player_chunks: Vec<(i32, i32)> = Vec::new();
        let mut chunk_seen = horizon_client::fast_hash::FxHashSet::default();
        for p in players {
            let key = (p.position.x.floor() as i32 >> 4, p.position.z.floor() as i32 >> 4);
            if chunk_seen.insert(key) {
                player_chunks.push(key);
            }
        }
        for (pcx, pcz) in player_chunks {
            for dx in -8..=8 {
                for dz in -8..=8 {
                    let key = (pcx + dx, pcz + dz);''',
        '''        let mut player_chunks: Vec<(i32, i32, i32)> = Vec::new();
        let mut chunk_seen = horizon_client::fast_hash::FxHashSet::default();
        for p in players {
            let reach = super::paper_fam_ev_entity::spawn_reach(p.id);
            let key = (p.position.x.floor() as i32 >> 4, p.position.z.floor() as i32 >> 4, reach);
            if chunk_seen.insert(key) {
                player_chunks.push(key);
            }
        }
        for (pcx, pcz, reach) in player_chunks {
            for dx in -reach..=reach {
                for dz in -reach..=reach {
                    let key = (pcx + dx, pcz + dz);'''),
    # A gathered player's world command waits for the regions (3b52f04de): the marker check first, as the port's.
    'worlds_engine#10': (
        '''            let c = host.commands.remove(0);
            if host.hub.guest(c.conn).is_some_and(|g| g.visiting)''',
        '''            let c = host.commands.remove(0);
            // A marker that came in during a command: what it's for is done here, not run as a command.
            if crate::game::paper_fam_ev_player::is_mark(&c.command) {
                crate::game::paper_fam_ev_player::run_held(self);
                continue;
            }
            if host.hub.guest(c.conn).is_some_and(|g| g.visiting)'''),
    # motion::Scene reads the level through world() (34e43cef9).
    'core_engine#1': (
        '''        if self.player.position.y < f64::from(self.scene.world().bottom_y() - 64) {
            let id = self
                .resources
                .health
                .damage
                .id("minecraft:out_of_world")
                .ok_or("missing environment damage type")?;
            self.player
                .hurt(&self.resources.health, self.scene.world().health_rules, id, 4.0)?;
        }''',
        '''        if let Some(amount) = crate::motion::void_hurt(self.player.position.y, self.scene.world().bottom_y()) {
            let id = self
                .resources
                .health
                .damage
                .id("minecraft:out_of_world")
                .ok_or("missing environment damage type")?;
            self.player
                .hurt(&self.resources.health, self.scene.world().health_rules, id, amount)?;
        }'''),
    # The console's regions come back before a command (resident regions): a plugin's console word first.
    'core_engine#22': (
        '''        let rest: Vec<&str> = words.collect();
        let lit = |s: &str| Text::literal(s.to_owned());
        // HorizonsX: every region still on its thread back first''',
        '''        if crate::game::paper_fam_core::console_word(self, line) {
            return;
        }
        let rest: Vec<&str> = words.collect();
        let lit = |s: &str| Text::literal(s.to_owned());
        // HorizonsX: every region still on its thread back first'''),
    # level_players is shared (an Arc) since 34e43cef9.
    'entity_engine#94': (
        '''    pub(super) fn level_players(&self) -> std::sync::Arc<[LevelPlayer]> {''',
        '''    /// `EntitySelector.PLAYER_AFFECTS_SPAWNING`: the level's players but those a plugin took out of it.
    fn spawn_players(&self) -> std::sync::Arc<[LevelPlayer]> {
        self.level_players_iter().filter(|p| !horizon_client::paper_hub::plugin_marked(3, p.id as u32)).collect()
    }
    pub(super) fn level_players(&self) -> std::sync::Arc<[LevelPlayer]> {'''),
    # spawn_players is shared as level_players is (an Arc).
    'entity_engine#138': (
        '''    fn spawn_players(&self) -> std::sync::Arc<[LevelPlayer]> {''',
        '''    pub(super) fn spawn_players(&self) -> std::sync::Arc<[LevelPlayer]> {'''),
    # The player state turn skips players whose region ran it (resident regions).
    'entity_engine#241': (
        '''                if self.here(&self.guests[i]) && !self.guests[i].turn_done {
                    self.sync_player_state(i);''',
        '''                if self.here(&self.guests[i]) && !self.guests[i].turn_done {
                    if crate::paper_hub::take_refresh(self.guests[i].entity_id) {
                        self.refresh_own(i, ctx);
                    }
                    self.sync_player_state(i);'''),
    # as_guest's swaps are timed (detail_span) since 34e43cef9.
    'ev_entity_engine#43': (
        '''        let swap = detail_span("as_guest_enter", || self.enter_guest(host, conn))?;
''',
        '''        let body = host.hub.guest(conn).map(|g| g.entity_id);
        let swap = detail_span("as_guest_enter", || self.enter_guest(host, conn))?;
        horizon_client::mobs::ee::bind(body);
'''),
    'ev_entity_engine#44': (
        '''        detail_span("as_guest_leave", || self.leave_guest(host, swap));
''',
        '''        horizon_client::mobs::ee::bind(None);
        detail_span("as_guest_leave", || self.leave_guest(host, swap));
'''),
    # Hub::guest_index (a50b3439f).
    'ev_player_engine#121': (
        '''            let Some(i) = host.hub.guest_index(t.conn) else {
                continue;
            };
            let source = self.dim;
''',
        '''            let Some(i) = host.hub.guest_index(t.conn) else {
                continue;
            };
            let Some(t) = crate::game::paper_fam_ev_player::level_tp_gate(self, host, t) else {
                continue;
            };
            let source = self.dim;
'''),
    # The game event's own radius (sculk.rs also asks it in anyone_hears now: that one stays vanilla's).
    'ev_world_engine#5': (
        '''        let at = block_of(pos);
        let radius = sculk::notification_radius(event).min(16);''',
        '''        let at = block_of(pos);
        let radius = super::paper_fam_ev_world::take_radius().unwrap_or_else(|| sculk::notification_radius(event).min(16));'''),
    # Block events are timed (tb_block_events) since 34e43cef9.
    'ev_world_engine#168': (
        '''                            .map(|(y, mask)| (*y, Some(mask.clone())))
                    });
                }
            });
            horizon_client::spans::time("tb_block_events", || b.run_block_events(level, ticks));
            b.tick_block_entities(level, ticks);
        });
''',
        '''                            .map(|(y, mask)| (*y, Some(mask.clone())))
                    });
                    if horizon_world::level::ASK_WAITING.load(std::sync::atomic::Ordering::Relaxed) {
                        asked = true;
                        break;
                    }
                }
            });
        });
        if !asked {
            break;
        }
        paper_fam_ev_world::run_asked(self);
        }
        loop {
            self.with_level(|b, level, ticks| {
                horizon_client::spans::time("tb_block_events", || b.run_block_events(level, ticks))
            });
            if !horizon_world::level::ASK_WAITING.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            paper_fam_ev_world::run_asked(self);
        }
        self.with_level(|b, level, ticks| b.tick_block_entities(level, ticks));
'''),
    # The mob list moves back whole (V2): the plugins hear the mobs' tick after it, as the port's row does.
    'ev_entity_engine#2': (
        '''            self.mobs.append(&mut mobs);
        }
        self.died = died;
''',
        '''            self.mobs.append(&mut mobs);
        }
        self.died = died;
        super::paper_fam_ev_entity::after_mobs(self);
'''),
    # V2's block entity map is an Fx map.
    'game_entity_engine#151': (
        '''#[derive(Default)]
pub struct BlockEntities {
    map: FxHashMap<Pos, BlockEntity>,
''',
        '''static NOTES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a server wants to hear what comes and goes.
pub fn hear_changes(listening: bool) {
    NOTES.store(listening, std::sync::atomic::Ordering::Relaxed);
}

#[derive(Default)]
pub struct BlockEntities {
    /// What came (`None`) and went (`Some`, what it was), in order. Only while a server listens.
    notes: Vec<(Pos, Option<BlockEntity>)>,
    map: FxHashMap<Pos, BlockEntity>,
'''),
    # V2's level hands its regions' spill back after the level (flush_spill_after_level).
    'items_engine#21': (
        '''        let result = f(self.res.behaviours(), &mut level, &mut self.block_ticks);
        self.flush_spill_after_level();
''',
        '''        let result = f(self.res.behaviours(), &mut level, &mut self.block_ticks);
        crate::game::paper_fam_items::take_gone(self);
        self.flush_spill_after_level();
'''),
}


def apply_rows(texts, label, rows, missing, skipped, own_ok=False):
    """Rows applied to `texts` (file under `crates` -> its text, read once by `family_edits`): an open
    costs ~2 ms here (the virus scanner), and reading and writing a file per row took 30 s a build."""
    for index, (name, old, new) in enumerate(rows):
        if f'{label}#{index}' in REPLACED:
            skipped.append(f'{label}#{index}(V2 row)')
            continue
        if f'{label}#{index}' in V2_FORMS:
            old, new = V2_FORMS[f'{label}#{index}']
        text = texts.read(name)
        # A speed or RAM row V2 took by hand is in already.
        if own_ok and new in text:
            skipped.append(f'{label}#{index}')
            continue
        count = text.count(old)
        span = None if count == 1 else LOOSE[0](text, old)
        if count == 1:
            texts.write(name, text.replace(old, new, 1))
        elif span:
            # The Paper engine's rule: whitespace, comments and trailing commas don't count (dim_patch.loose_span).
            texts.write(name, text[:span[0]] + new + text[span[1]:])
        elif own_ok:
            skipped.append(f'{label}#{index}(V2 own)')
        else:
            missing.append(f'{label}#{index} {name}: found {count} times\n    {old[:160]!r}')


class Texts:
    """The source files the rows change, each read once and written once at the end."""

    def __init__(self, crates):
        self.crates, self.texts, self.changed = crates, {}, set()

    def read(self, name):
        # One file, one text, however a row spells it (`game/../game.rs` is `game.rs`).
        name = os.path.normpath(name)
        if name not in self.texts:
            self.texts[name] = (self.crates / name).read_text(encoding='utf-8')
        return self.texts[name]

    def write(self, name, text):
        name = os.path.normpath(name)
        self.texts[name] = text
        self.changed.add(name)

    def save(self):
        for name in sorted(self.changed):
            (self.crates / name).write_text(self.texts[name], encoding='utf-8', newline='\n')


def family_edits(crates):
    """dim_patch.family_edits, but a row already in V2 is skipped, then V2's own rows (paper_v2_rows.py)."""
    skipped, missing = [], []
    texts = Texts(crates)
    for table in sorted(FAMILY.glob('*_engine.py')):
        if table.stem == 'ramprobe_engine':
            continue
        # V2 carries every row of these (taken earlier, some by hand in V2's own form): an insertion
        # row's text is still there, so applying it again would add its code twice.
        if table.stem in V2_OWN:
            skipped.append(f'{table.stem} (V2 has all)')
            continue
        scope = {}
        exec(compile(table.read_text(encoding='utf-8'), str(table), 'exec'), scope)
        apply_rows(texts, table.stem, scope.get('EDITS', []), missing, skipped)
    v2_rows = load(pathlib.Path(__file__).with_name('paper_v2_rows.py'), 'paper_v2_rows')
    apply_rows(texts, 'v2', v2_rows.EDITS, missing, skipped)
    texts.save()
    print(len(skipped), 'rows skipped:', ', '.join(skipped), flush=True)
    if missing:
        sys.exit('rows that do not fit V2:\n' + '\n'.join(missing))


def main():
    name = sys.argv[1]
    check = '--check' in sys.argv
    stage_only = '--stage-only' in sys.argv
    started = time.monotonic()
    engine = load(ENGINE / 'tools' / 'build_engine.py', 'build_engine')
    dim = load(ENGINE / 'tools' / 'dim_patch.py', 'dim_patch')
    LOOSE[0] = dim.loose_span
    last = [started]

    def lap(step):
        # Each step's own time, so a slow build says which part was slow.
        now = time.monotonic()
        print(f'{step}: {now - last[0]:.1f} s', flush=True)
        last[0] = now

    tree = WORK / 'tree'
    copy_tree(tree)
    lap('copy the worktree')
    (tree / '.horizon-revision').write_text(COMMIT, encoding='utf-8')
    mods = WORK / 'bundles'
    shutil.rmtree(mods, ignore_errors=True)
    for bundle in BUNDLES:
        shutil.copytree(ENGINE / 'bundle' / bundle, mods / bundle, ignore=shutil.ignore_patterns('Cargo.lock', 'target'))
    lap('bundle')
    fresh = WORK / 'fresh'
    shutil.rmtree(fresh, ignore_errors=True)
    tool = os.environ.get('HORIZON_MIXINS') or shutil.which('horizon-mixins')
    if not tool:
        sys.exit('horizon-mixins not found: cargo install --git https://github.com/horizon-rust/horizon-mixins')
    applied = subprocess.run([str(tool), 'apply', '--workspace', str(tree), '--mods', str(mods), '--out', str(fresh)],
                             capture_output=True, text=True, errors='replace')
    if applied.returncode:
        sys.exit('horizon-mixins failed:\n' + (applied.stdout + applied.stderr)[-4000:])
    lap('horizon-mixins apply')
    dim.family_edits = family_edits
    sys.argv = [sys.argv[0], str(fresh)]
    dim.main()
    lap('dim_patch and family rows')
    stage = WORK / 'stage'
    engine.sync(fresh, stage)
    lap('sync to the stage')
    if stage_only:
        # The patched tree only (../build/horizonsx-paper/stage), for another build of it (tools/v2/build_linux_paper.sh).
        print('stage ready', stage, flush=True)
        return
    command = ['cargo', '+1.99.1', 'check' if check else 'build', '--locked', '--profile', 'fast',
               '--no-default-features', '-p', 'horizon-client', '--bin', 'horizon-client']
    if os.name == 'nt':
        command += ['--features', 'windows-segment-heap']
    env = dict(os.environ, CARGO_TARGET_DIR=str(CACHE))
    # Compile speed only, the program is the same: horizon-client's front end (type checking...) on 8 threads
    # (rustc's parallel front end; RUSTC_BOOTSTRAP lets this toolchain take -Z). HX_NO_THREADS=1 turns it off.
    rustflags = [] if os.environ.get('HX_NO_THREADS') else ['-Zthreads=8']
    if '--quick' in sys.argv:
        # Iteration builds only, never for perf numbers or the VPS: horizon-client unoptimized in many small units
        # (an engine edit rebuilds ~3x faster; the program runs slower). Its own cache, so a normal build stays warm.
        command += ['--config', 'profile.fast.package.horizon-client.opt-level=0',
                    '--config', 'profile.fast.package.horizon-client.codegen-units=256']
        env['CARGO_TARGET_DIR'] = str(CACHE) + '-quick'
    if rustflags:
        import json
        command += ['-Zprofile-rustflags', '--config', 'profile.fast.package.horizon-client.rustflags=' + json.dumps(rustflags)]
        env['RUSTC_BOOTSTRAP'] = '1'
    print('compiling', flush=True)
    # Below normal priority, so the PC stays usable while it compiles.
    low = {'creationflags': BELOW_NORMAL} if os.name == 'nt' else {'preexec_fn': lambda: os.nice(5)}
    subprocess.run(command, cwd=stage, env=env, check=True, **low)
    print(f'done in {time.monotonic() - started:.0f} s', flush=True)
    if check:
        return
    folder = PERF / name
    folder.mkdir(parents=True, exist_ok=True)
    built = pathlib.Path(env['CARGO_TARGET_DIR']) / 'fast' / f'horizon-client{EXE}'
    shutil.copy2(built, folder / f'horizon-client{EXE}')
    for tool_name in (f'horizon-server{EXE}', f'horizon-bots{EXE}'):
        if (PERF / 'before' / tool_name).is_file():
            shutil.copy2(PERF / 'before' / tool_name, folder / tool_name)
    print('built', folder, flush=True)


if __name__ == '__main__':
    main()
