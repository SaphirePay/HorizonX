//! `Commandbreak`.
use crate::prelude::*;
use horizon_bukkit::fam_core::get_plugin_manager;
use horizon_bukkit::fam_ev_world::BlockBreakEvent;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("break") };

fn run(user: &User, _label: &str, _args: &[String]) -> EssResult {
    let block = user.get_target_block(20).filter(|block| !block.get_type().is_air()).ok_or_else(|| EssError::NoCharge)?;
    fail_if!(block.get_type().name() == "BEDROCK" && !user.is_authorized("essentials.break.bedrock"), EssError::tl("noBreakBedrock", &[]));
    let mut event = user.player().and_then(|player| BlockBreakEvent::new(&block, &player)).ok_or_else(|| EssError::NoCharge)?;
    get_plugin_manager().call_event(&mut event);
    fail_if!(event.is_cancelled(), EssError::NoCharge);
    block.set_type(&Material::of("AIR"));
    Ok(())
}
