# The players as plugins read them while a hook holds the server's hub itself. A guest's command is heard before it
# runs (PlayerCommandPreprocessEvent, guest_command's start) with the hub taken out of the game: the players' state
# (op level, place, game mode) read nobody (xwojteek-core's command filter found its op without permissions and
# refused every command). The hook keeps what they were for its listeners (paper_bridge.on_command_preprocess).
GAME = 'horizon-client/src/game/'

EDITS = [
    (GAME + 'native_server.rs', '''    pub(super) fn native_server_players(&self) -> Vec<NativeServerPlayer> {
        let Some(host) = &self.lan else {
            return Vec::new();
        };
        host.hub
            .guests''', '''    pub(super) fn native_server_players(&self) -> Vec<NativeServerPlayer> {
        let Some(host) = &self.lan else {
            return HELD_PLAYERS.with(|held| held.borrow().clone());
        };
        Self::native_players_of(host)
    }
    /// The placed guests of a server as server hooks see them.
    pub(crate) fn native_players_of(host: &LanHost) -> Vec<NativeServerPlayer> {
        host.hub
            .guests'''),
    (GAME + 'native_server.rs', '''/// A player on the server, as a server hook sees them.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeServerPlayer {''', '''thread_local! {
    /// The players while a hook holds the hub (`native_server_players` has no `lan` then): set for that hook's
    /// listeners only (`hold_players`).
    pub(crate) static HELD_PLAYERS: std::cell::RefCell<Vec<NativeServerPlayer>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// `then`, with the players of `host` readable by plugins while the hub is out of the game.
pub(crate) fn hold_players<T>(host: &LanHost, then: impl FnOnce() -> T) -> T {
    let before = HELD_PLAYERS.with(|held| std::mem::replace(&mut *held.borrow_mut(), Game::native_players_of(host)));
    let out = then();
    HELD_PLAYERS.with(|held| *held.borrow_mut() = before);
    out
}

/// A player on the server, as a server hook sees them.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeServerPlayer {'''),
]
