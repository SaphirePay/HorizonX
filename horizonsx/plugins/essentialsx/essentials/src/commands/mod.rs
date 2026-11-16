//! The commands. A command is one file here: `build.rs` finds it, lists it in the table below, and
//! keeps its entry in `plugin.yml`. See COMMANDS.md. Nothing in this file changes to add one.
pub mod base;

/// A toggle command's `COMMAND`: all that's its own is `toggle_player`.
macro_rules! toggle_command {
    ($name:literal, $others:literal) => {
        pub const COMMAND: super::base::Command = super::base::Command {
            run_user: Some(|user, _, args| super::base::handle_toggle_with_args(user, args, $others, toggle_player)),
            run_source: Some(|sender, _, args| super::base::toggle_other_players(sender, args, toggle_player)),
            tab_user: Some(|user, _, args| super::base::toggle_tab_user(user, args, $others)),
            tab_source: Some(|sender, _, args| super::base::toggle_tab_source(sender, args)),
            ..super::base::Command::new($name)
        };
    };
}

include!(concat!(env!("OUT_DIR"), "/command_table.rs"));

pub use base::register;
