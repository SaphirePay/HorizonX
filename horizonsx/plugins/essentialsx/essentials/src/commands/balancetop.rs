//! `Commandbalancetop`.
use crate::prelude::*;
use crate::ess::play_one_tick_of;
use crate::textreader::{SimpleTextInput, TextPager};
use chrono::{Local, TimeZone};
use std::sync::Mutex;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("balancetop") };

const MIN_USERS: usize = 50;
const CACHE_TIME: i64 = 2 * 60 * 1000;
static CACHE: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn output_cache(sender: &CommandSource, page: i32) {
    let shown = Local.timestamp_millis_opt(ess().balance_top().get_cache_age()).single().map(|date| date.format("%-m/%-d/%y, %-I:%M %p").to_string()).unwrap_or_default();
    sender.send_tl("balanceTop", crate::args![shown]);
    let input = SimpleTextInput { lines: CACHE.lock().unwrap_or_else(|e| e.into_inner()).clone(), ..Default::default() };
    TextPager { lines: &input.lines, chapters: &input.chapters, bookmarks: &input.bookmarks, one_page: false }.show_page(Some(&page.to_string()), None, Some("balancetop"), sender);
}

fn may_force(sender: &CommandSource) -> bool {
    sender.get_user().is_none_or(|user| user.is_authorized("essentials.balancetop.force"))
}

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let mut page = 0;
    let mut force = false;
    if let Some(first) = args.first() {
        match first.parse::<i32>() {
            Ok(number) => page = number,
            Err(_) => force = first.eq_ignore_ascii_case("force") && may_force(sender),
        }
    }
    if !force && ess().balance_top().get_cache_age() > now_ms() - CACHE_TIME {
        output_cache(sender, page);
        return Ok(());
    }
    let count = ess().users().get_user_count();
    if count > MIN_USERS {
        sender.send_tl("orderBalances", crate::args![count]);
    }
    let sender = sender.clone();
    ess().run_task_asynchronously(move || view(sender, page, force));
    Ok(())
}

fn view(sender: CommandSource, page: i32, force: bool) {
    if settings().is_eco_disabled() {
        if settings().is_debug() {
            crate::info("Internal economy functions disabled, aborting baltop.");
        }
        return;
    }
    let top = ess().balance_top();
    if force || top.is_cache_locked() || top.get_cache_age() <= now_ms() - CACHE_TIME {
        top.calculate_async(move || {
            rebuild_cache();
            output_cache(&sender, page);
        });
    } else {
        let mut sender = Some(sender);
        ess().schedule_sync_delayed_task(move || {
            if let Some(sender) = sender.take() {
                output_cache(&sender, page);
            }
        });
    }
}

fn rebuild_cache() {
    let top = ess().balance_top();
    let total = top.get_balance_top_total();
    let mut lines = vec![mini_to_legacy(&tl_literal("serverTotal", crate::args![parsed(display_currency(&total))]))];
    for (index, entry) in top.get_balance_top_cache().into_iter().enumerate() {
        let playtime = ess().get_user_by_uuid(&entry.uuid).map_or(0, |user| play_one_tick_of(&user));
        let seconds = (playtime / 20).max(0);
        if (settings().show_zero_baltop() || entry.balance > BigDecimal::zero()) && entry.balance >= settings().get_baltop_min_balance() && seconds >= settings().get_baltop_min_playtime() {
            lines.push(mini_to_legacy(&tl_literal("balanceTopLine", crate::args![index + 1, entry.display_name.clone(), parsed(display_currency(&entry.balance))])));
        }
    }
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = lines;
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    if args.len() != 1 {
        return Some(Vec::new());
    }
    let mut options = vec!["1".to_owned()];
    if may_force(sender) {
        options.push("force".to_owned());
    }
    Some(options)
}
