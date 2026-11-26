"""Horizon V2's own rows for the Paper plugin layer, applied by paper_build.py after the port's rows.

Each row is (file under `crates`, text, replacement), as plugin-tools/tools/family/*_engine.py, the text
as horizon-mixins leaves it (formatted). They fill in V2's hook points so its multi-core parts give way
to plugins: a part whose plugin events a plugin listens to runs on the game thread, as Paper's do.
"""

# Mob ticks: every event a mob's own tick can fire. Any heard, or a mob effect's or a hit's change
# waiting for its listeners: the serial loop, where those events run.
MOB_HOOKS = '''fn v2_plugin_mob_hooks() -> bool {
    let heard: Vec<String> = if horizon_mod_runtime::listening() {
        horizon_mod_runtime::listened()
            .into_iter()
            .filter(|event| event.starts_with("ee_") || event == "allow_damage")
            .collect()
    } else {
        Vec::new()
    };
    let on = horizon_client::paper_hub::effects_heard() || horizon_client::paper_hub::v2_hits_heard()
        || !heard.is_empty();
    // Said once, for whoever runs the server: why the mobs tick on one core.
    static TOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if on && !TOLD.swap(true, std::sync::atomic::Ordering::Relaxed) {
        log::info!("[v2] mob ticks run on the game thread: plugins listen to mob events {heard:?}");
    }
    on
}'''

EDITS = [
    # HorizonsX's resident regions: the plugins' chat answers (ChatProcessor.complete) go out from the server's thread to
    # every player: a gathered player's (`Guest::visiting`) each tick, whatever its inbox holds; a region thread
    # never sends them (it reached only its own players, and the line came to the log much later).
    ('horizon-client/src/lan/hub.rs', '''            } else if self.guests[i].visiting
                && self.guests[i].inbox.front().is_some_and(|(p, _)| !region_local(p))
            {
                self.process_inbox(i, ctx, &mut events);
            }''', '''            } else if self.guests[i].visiting {
                self.chat_answered(i, &mut events);
                if self.guests[i].inbox.front().is_some_and(|(p, _)| !region_local(p)) {
                    self.process_inbox(i, ctx, &mut events);
                }
            }'''),
    ('horizon-client/src/lan/hub.rs', '''        let region = self.region;
        self.chat_answered(i, events);''', '''        let region = self.region;
        if !region {
            self.chat_answered(i, events);
        }'''),
    # HorizonsX's resident regions: a plugin's question about a place or an entity a parked region holds is that
    # region's to answer (`Game::answer_where_held`).
    ('horizon-client/src/game.rs', '''        runtime::borrowed(|game: &mut Game<'static>| crate::game::paper_query::answer(
                game,
                request,
            ))
            .or_else(|| crate::game::paper_fam_ev_entity::free_answer(request))''', '''        runtime::borrowed(|game: &mut Game<'static>| game.answer_where_held(request, crate::game::paper_query::answer))
            .or_else(|| crate::game::paper_fam_ev_entity::free_answer(request))'''),
    # HorizonsX's resident regions: a chunk a region holds on its thread never loads here (streaming leaves it out),
    # so a plugin's box over one waited the whole minute and froze the server: the regions come back next tick
    # instead, and the plugin's next try finds the chunks here.
    ('horizon-client/src/game.rs', '''        if game.region_worker || game.regions_lending {
            return false;
        }
        let added: Vec<(i32, i32)> = wanted''', '''        if game.region_worker || game.regions_lending {
            return false;
        }
        if wanted.iter().any(|c| game.streaming.away.contains(c)) {
            game.resident_land = true;
            return false;
        }
        let added: Vec<(i32, i32)> = wanted'''),
    # The plugin goal's two calls (entity_engine's row on the trait, which V2 made `Send`).
    ('horizon-client/src/mobs/goals.rs',
     'pub trait Goal: std::fmt::Debug + Send {',
     '''pub trait Goal: std::fmt::Debug + Send {
    /// A plugin's goal: its key.
    fn plugin_key(&self) -> Option<&str> {
        None
    }
    /// A plugin's goal: whether its code wants to run.
    fn set_wanted(&self, _wanted: bool) {}'''),
    # Whether a hit waits for its listeners: read by the mob hook below.
    ('horizon-client/src/lib.rs',
     '''    pub fn holding_hits(on: bool) {
        HOLDING_HITS.set(on);
    }
''',
     '''    pub fn holding_hits(on: bool) {
        HOLDING_HITS.set(on);
    }
    /// Horizon V2: whether a listener hears hits on mobs (they wait for it in a mob's tick).
    pub fn v2_hits_heard() -> bool {
        HITS_HEARD.load(std::sync::atomic::Ordering::Relaxed)
    }
'''),
    ('horizon-client/src/game/creatures.rs',
     '''fn v2_plugin_mob_hooks() -> bool {
    false
}''',
     MOB_HOOKS),
    # The players that count for mobs: those a plugin hasn't taken out (`setAffectsSpawning`).
    ('horizon-client/src/game/creatures.rs',
     'let region_players = &players;',
     'let region_players = &spawn_players;'),
    # Natural spawning: `PreCreatureSpawnEvent` is asked from inside an attempt.
    ('horizon-client/src/game/natural_spawning.rs',
     '''fn v2_plugin_spawn_hooks() -> bool {
    false
}''',
     '''fn v2_plugin_spawn_hooks() -> bool {
    let on = horizon_client::mobs::ee::hears("ee_pre_spawn");
    static TOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if on && !TOLD.swap(true, std::sync::atomic::Ordering::Relaxed) {
        log::info!("[v2] natural spawning runs on the game thread: a plugin listens to PreCreatureSpawnEvent");
    }
    on
}'''),
    # Random ticks: a block change a listener has a say in is answered between chunks.
    ('horizon-client/src/game.rs',
     '''fn v2_asks_waiting() -> bool {
    false
}''',
     '''fn v2_asks_waiting() -> bool {
    horizon_world::level::ASK_WAITING.load(std::sync::atomic::Ordering::Relaxed)
}'''),
    ('horizon-client/src/game.rs',
     'fn v2_answer_asks(&mut self) {}',
     '''fn v2_answer_asks(&mut self) {
        paper_fam_ev_world::run_asked(self);
    }'''),
]
