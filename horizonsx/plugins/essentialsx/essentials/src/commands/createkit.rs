//! `Commandcreatekit`.
use crate::prelude::*;
use crate::utils::base64::encode_lines;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("createkit") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let [name, delay] = args else {
        return Err(EssError::not_enough());
    };
    let delay: i64 = delay.parse().map_err(|_| EssError::Message(format!("For input string: \"{delay}\"")))?;
    let Some(player) = user.player() else { return Ok(()) };
    let use_bytes = settings().is_use_better_kits();
    let list: Vec<String> = inventories::get_inventory(&player, true).iter().enumerate().filter_map(|(slot, item)| item.as_ref().filter(|item| !item.get_type().is_air()).map(|item| {
        if use_bytes { format!("slot:{slot} @{}", encode_lines(&item.serialize_as_bytes())) } else { format!("slot:{slot} {}", item_db().serialize(item)) }
    })).collect();
    if settings().is_pastebin_create_kit() {
        upload_paste(user.get_source(), name.clone(), delay, list);
        return Ok(());
    }
    ess().kits().add_kit(name, &list, delay);
    user.send_tl("createdKit", crate::args![name, list.len(), delay]);
    Ok(())
}

/// `uploadPaste`: the kit as `config.yml` would hold it.
fn upload_paste(sender: CommandSource, name: String, delay: i64, list: Vec<String>) {
    ess().run_task_asynchronously(move || {
        let kit = serde_yaml::to_value(serde_json::json!({ "kits": { name.as_str(): { "delay": delay, "items": list } } })).and_then(|kit| serde_yaml::to_string(&kit));
        let contents = kit.map(|kit| format!("# Copy the kit code below into the kits section in your config.yml file\n{kit}"));
        let result = contents.map_err(|error| error.to_string()).and_then(|contents| crate::paste::create_paste(&[(format!("kit_{name}.yml"), contents)]));
        ess().schedule_sync_delayed_task(move || match result {
            Ok((_, url)) => {
                let separator = tl_literal("createKitSeparator", &[]);
                let delay_format = if delay <= 0 { "0".to_owned() } else { format_date_diff(now_ms() + delay * 1000) };
                sender.send_message(&mini_to_legacy(&separator));
                sender.send_tl("createKitSuccess", crate::args![name, delay_format, url.clone()]);
                sender.send_message(&mini_to_legacy(&separator));
                if settings().is_debug() {
                    crate::info(&format!("{} created a kit: {url}", sender.get_sender().get_name()));
                }
            }
            Err(error) => {
                sender.send_tl("createKitFailed", crate::args![name]);
                crate::severe(&format!("Error creating kit: {error}"));
            }
        });
    });
}
