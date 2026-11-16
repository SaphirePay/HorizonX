#!/usr/bin/env python3
"""Writes horizon-bukkit's `prelude`: one `use horizon_bukkit::prelude::*;` brings every event type and the classes a
plugin uses most, so nobody (no person, no AI) has to know which of the SDK's modules a type lives in.

Usage: python -I tools/sdk_prelude.py [OUT.rs]   (default sdk/horizon-bukkit/src/prelude.rs)
An event two modules define is taken from the one with the most `/// bukkit:` tags (the fuller one).
"""
import collections
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / 'sdk' / 'horizon-bukkit' / 'src'
EVENT_IMPL = re.compile(r'^impl (?:crate::event::)?Event for ([A-Z]\w*)\b', re.M)
# Every public event type, including those a macro implements (`chat_event!(AsyncPlayerChatEvent, ...)`).
PUB_EVENT = re.compile(r'^pub (?:struct|enum) ([A-Z]\w*Event)\b', re.M)
# Where two modules define an event, the one a plugin should use (the other is the converter's).
PREFER = {'PlayerDeathEvent': 'event'}

# The classes and functions a plugin reaches for first, by the path they have.
COMMON = [
    'crate::{Plugin, Player, CommandSender, World, Location, ItemStack, Inventory, BossBar, Material, Block, OfflinePlayer, Value, bukkit_plugin, embed_resources}',
    'crate::{get_player, get_online_players, get_worlds, get_world, broadcast_message, chat_color}',
    'crate::chat_color::translate_alternate_color_codes',
    'crate::fam_core::BukkitTask',
    'crate::adventure::Component',
    'crate::config::YamlConfiguration',
    'crate::scheduler::{run_task, run_task_later, run_task_timer, run_task_asynchronously, cancel_task}',
    'crate::enums',
    'crate::serde_json::{self, json}',
]


def main():
    out = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else SRC / 'prelude.rs'
    found = collections.defaultdict(list)  # event -> [(tags, module)]
    for path in sorted(SRC.glob('*.rs')):
        if path.stem in ('prelude', 'lib'):
            continue
        text = path.read_text(encoding='utf-8', errors='replace')
        for name in sorted(set(EVENT_IMPL.findall(text)) | set(PUB_EVENT.findall(text))):
            # Only what a plugin can name: a public type.
            if not re.search(rf'^pub (?:struct|enum) {name}\b', text, re.M):
                continue
            tags = len(re.findall(rf'bukkit~?: [\w/]+/{name}\.', text))
            found[name].append((tags, path.stem))
    lib = (SRC / 'lib.rs').read_text(encoding='utf-8', errors='replace')
    for name in EVENT_IMPL.findall(lib):
        if not re.search(rf'^pub (?:struct|enum) {name}\b', lib, re.M):
            continue
        found[name].append((len(re.findall(rf'bukkit~?: [\w/]+/{name}\.', lib)), None))
    by_module = collections.defaultdict(list)
    for name, places in sorted(found.items()):
        tags, module = max(places, key=lambda p: (p[1] == PREFER.get(name), p[0]))
        by_module[module].append(name)
    lines = [
        '//! Written by tools/sdk_prelude.py: do not edit.',
        '//!',
        '//! `use horizon_bukkit::prelude::*;` brings every event type (whatever module defines it) and the classes',
        '//! a plugin uses most: no need to know where in the SDK a type lives. API_INDEX.md maps every Bukkit/Paper',
        '//! method to its Rust function.',
        '#![allow(unused_imports, ambiguous_glob_reexports)]',
        '',
    ]
    lines += [f'pub use {c};' for c in COMMON]
    for module, names in sorted(by_module.items(), key=lambda kv: kv[0] or ''):
        path = 'crate' if module is None else f'crate::{module}'
        lines.append(f'pub use {path}::{{{", ".join(names)}}};')
    out.write_text('\n'.join(lines) + '\n', encoding='utf-8')
    print(f'{sum(len(v) for v in by_module.values())} events from {len(by_module)} modules -> {out}')


if __name__ == '__main__':
    main()
