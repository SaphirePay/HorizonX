//! The in-game editor (`/axda editor <auction>`, `/axda schedulers`): MainEditor, NPCEditor, RewardEditor,
//! RewardSettingEditor, RewardItemsEditor, SchedulerEditor and Scheduler2Editor as chest menus. Every change is
//! written to the auction's file (or schedulers.yml) and read back at once. Text is typed in chat; a location is
//! where the player stands when they type `done`. Where the Java editor takes the item on the cursor, this one
//! takes the item in the main hand.

use crate::auction;
use horizon_bukkit::{ItemStack, Player};
use horizon_kit::gui::{self, Click, Menu};
use serde_yaml::{Mapping, Value as Yaml};

const FRAME: &str = "PURPLE_STAINED_GLASS_PANE";

fn title(auction: &str) -> String {
    crate::texts::legacy(&format!("&0Editor > &l{auction}"))
}

fn icon(material: &str, name: &str, lore: &[String]) -> ItemStack {
    let lines: Vec<&str> = lore.iter().map(String::as_str).collect();
    gui::icon(material, name, &lines)
}

fn frame(menu: &mut Menu) {
    let pane = gui::filler(FRAME);
    for s in (0..9).chain(45..54) {
        if s < menu.size() {
            menu.item(s, pane.clone());
        }
    }
}

fn current(value: &str) -> String {
    format!("&#D200FF&l> &#FF00FFCurrent value: &f{value}")
}

/// Changes the auction's file, saves it, reads the auction again.
fn edit(name: &str, change: impl FnOnce(&mut horizon_kit::Config)) {
    auction::with(name, |a| {
        change(&mut a.cfg);
        a.cfg.save();
        a.reload();
    });
}

/// The next chat line, unless it is `cancel` (then the editor just comes back).
fn ask_text(p: &Player, message: &str, back: impl Fn(&Player) + Send + 'static, then: impl FnOnce(&Player, String) + Send + 'static) {
    p.close_inventory();
    crate::texts::send(p, message);
    crate::ask(p, move |p, line| {
        if !line.trim().eq_ignore_ascii_case("cancel") {
            then(p, line);
        }
        back(p);
    });
}

fn location_line(at: Option<&horizon_bukkit::Location>) -> String {
    at.map_or_else(|| "---".to_owned(), |l| {
        let t = |v: f64| ((v * 100.0) as i64) as f64 / 100.0;
        format!("{};{};{};{};{};{}", l.world.get_name(), t(l.x), t(l.y), t(l.z), t(f64::from(l.yaw)), t(f64::from(l.pitch)))
    })
}

fn int_lore() -> [String; 5] {
    [" ".into(), "&#D200FF&l> &#D200FFLeft Click &8- &#00FF00+1".into(), "&#D200FF&l> &#D200FFRight Click &8- &#FF0000-1".into(), "&#D200FF&l> &#D200FFShift + Left Click &8- &#00FF00+10".into(), "&#D200FF&l> &#D200FFShift + Right Click &8- &#FF0000-10".into()]
}

fn step(click: Click) -> i64 {
    match click {
        Click::Left => 1,
        Click::Right => -1,
        Click::ShiftLeft => 10,
        Click::ShiftRight => -10,
        _ => 0,
    }
}

/// An item as an auction file keeps it (ItemBuilder.serialize): material, amount, name, lore.
fn serialize(stack: &ItemStack) -> Yaml {
    let mut m = Mapping::new();
    m.insert("material".into(), Yaml::String(stack.get_type().name()));
    if stack.get_amount() != 1 {
        m.insert("amount".into(), Yaml::Number(stack.get_amount().into()));
    }
    if let Some(name) = stack.get_display_name() {
        m.insert("name".into(), Yaml::String(name.replace('§', "&")));
    }
    let lore = stack.get_lore();
    if !lore.is_empty() {
        m.insert("lore".into(), Yaml::Sequence(lore.into_iter().map(|l| Yaml::String(l.replace('§', "&"))).collect()));
    }
    Yaml::Mapping(m)
}

fn held(p: &Player) -> Option<ItemStack> {
    p.get_inventory().get_item_in_main_hand().filter(|s| !s.is_empty())
}

// ---- MainEditor ----

pub fn main(p: &Player, name: &str) {
    let Some((spawn, display, display_on, display_item, min, max)) = auction::with(name, |a| {
        let item = a.cfg.at("display.block").map(|v| horizon_kit::item::Spec::from_yaml(v).material.to_ascii_uppercase());
        (a.spawn.clone(), a.display.clone(), a.cfg.bool("display.enabled", true), item, a.cfg.int("players.minimum", 0), a.cfg.int("players.maximum", -1))
    }) else {
        return;
    };
    let mut m = Menu::new(6, &title(name));
    frame(&mut m);
    let n = name.to_owned();
    let loc_lore = |at: Option<&horizon_bukkit::Location>| vec![" ".to_owned(), current(&location_line(at)), " ".into(), "&#D200FF&l> &#D200FFClick &8- &#00FF00Set Location".into()];
    {
        let n = n.clone();
        m.button(19, icon("MAP", "&#D200FF&lJoin Location", &loc_lore(spawn.as_ref())), move |c| {
            let n2 = n.clone();
            let n3 = n.clone();
            ask_text(&c.player, "&#FF00FFGo to the location! &#DDDDDD(write &#FF00FFdone&#DDDDDD to select or &#FF00FFcancel &#DDDDDDto stop)", move |p| main(p, &n2), move |p, _| {
                let at = p.get_location();
                edit(&n3, |cfg| cfg.set("locations.spawn", Yaml::String(auction::write_location(&at))));
            });
        });
    }
    {
        let n = n.clone();
        m.button(21, icon("MAP", "&#D200FF&lDisplay Block Location", &loc_lore(display.as_ref())), move |c| {
            let n2 = n.clone();
            let n3 = n.clone();
            ask_text(&c.player, "&#FF00FFGo to the location! &#DDDDDD(write &#FF00FFdone&#DDDDDD to select or &#FF00FFcancel &#DDDDDDto stop)", move |p| main(p, &n2), move |p, _| {
                let at = p.get_location();
                let block = horizon_bukkit::Location::new(at.world.clone(), at.x.floor(), at.y.floor(), at.z.floor(), 0.0, 0.0);
                edit(&n3, |cfg| cfg.set("locations.display", Yaml::String(auction::write_location(&block))));
            });
        });
    }
    for (slot, route, label) in [(23, "entrance", "&#D200FF&lEntrance NPC"), (24, "exit", "&#D200FF&lExit NPC"), (25, "auctioneer", "&#D200FF&lAuctioneer NPC")] {
        let n = n.clone();
        m.button(slot, icon("PLAYER_HEAD", label, &[" ".into(), "&#D200FF&l> &#D200FFClick &8- &#D200FFOpen Menu".into()]), move |c| npc(&c.player, &n, route));
    }
    {
        let n = n.clone();
        let lore = vec![
            " ".into(),
            format!("&#D200FF&l> &#FF00FFEnabled: &f{display_on}"),
            format!("&#D200FF&l> &#FF00FFItem: &f{}", display_item.unwrap_or_else(|| "---".into())),
            " ".into(),
            "&#D200FF&l> &#D200FFClick While Holding Item &8- &#D200FFReplace current one".into(),
            "&#D200FF&l> &#D200FFShift + Click &8- &#D200FFEnable/Disable display".into(),
        ];
        m.button(28, icon("BLACK_STAINED_GLASS", "&#D200FF&lDisplay Block", &lore), move |c| {
            if c.click.is_shift() {
                edit(&n, |cfg| {
                    let on = cfg.bool("display.enabled", true);
                    cfg.set("display.enabled", Yaml::Bool(!on));
                });
            } else if let Some(stack) = held(&c.player) {
                edit(&n, |cfg| cfg.set("display.block", serialize(&stack.as_one())));
            } else {
                crate::tell_lang(&c.player, "errors.hold-something", &[]);
                return;
            }
            main(&c.player, &n);
        });
    }
    for (slot, path, label, value, floor) in [(30, "players.minimum", "&#D200FF&lMinimum required players", min, 0), (31, "players.maximum", "&#D200FF&lMaximum required players", max, -1)] {
        let n = n.clone();
        let mut lore = if floor < 0 { vec!["&#DDDDDD-1 = disable limit".to_owned()] } else { Vec::new() };
        lore.extend([" ".into(), current(&value.to_string())]);
        lore.extend(int_lore());
        m.button(slot, icon("TOTEM_OF_UNDYING", label, &lore), move |c| {
            let by = step(c.click);
            edit(&n, |cfg| {
                let v = cfg.int(path, value);
                cfg.set(path, Yaml::Number((v + by).max(floor).into()));
            });
            main(&c.player, &n);
        });
    }
    {
        let n = n.clone();
        m.button(34, icon("GOLD_INGOT", "&#D200FF&lRewards", &[" ".into(), "&#D200FF&l> &#D200FFClick &8- &#D200FFOpen Menu".into()]), move |c| rewards(&c.player, &n, 0));
    }
    m.open(p);
}

// ---- NPCEditor ----

pub fn npc(p: &Player, name: &str, route: &'static str) {
    let base = format!("npcs.{route}");
    let Some((on, at, lines)) = auction::with(name, |a| (a.cfg.bool(&format!("{base}.enabled"), true), horizon_kit::yaml::string(&a.cfg.doc, &format!("{base}.location")).and_then(|s| auction::parse_location(&s)), a.cfg.list(&format!("{base}.name")))) else {
        return;
    };
    let mut m = Menu::new(6, &title(name));
    frame(&mut m);
    let n = name.to_owned();
    {
        let (n, base) = (n.clone(), base.clone());
        let lore = vec![" ".into(), current(&on.to_string()), " ".into(), "&#D200FF&l> &#D200FFClick &8- &#D200FFToggle".into()];
        m.button(4, icon(if on { "LIME_DYE" } else { "GRAY_DYE" }, "&#D200FF&lEnabled", &lore), move |c| {
            edit(&n, |cfg| cfg.set(&format!("{base}.enabled"), Yaml::Bool(!on)));
            npc(&c.player, &n, route);
        });
    }
    {
        let (n, base) = (n.clone(), base.clone());
        let lore = vec![" ".into(), current(&location_line(at.as_ref())), " ".into(), "&#D200FF&l> &#D200FFClick &8- &#00FF00Set Location".into()];
        m.button(28, icon("MAP", "&#D200FF&lNPC Location", &lore), move |c| {
            let n2 = n.clone();
            let (n3, base) = (n.clone(), base.clone());
            ask_text(&c.player, "&#FF00FFGo to the location! &#DDDDDD(write &#FF00FFdone&#DDDDDD to select or &#FF00FFcancel &#DDDDDDto stop)", move |p| npc(p, &n2, route), move |p, _| {
                let here = p.get_location();
                edit(&n3, |cfg| cfg.set(&format!("{base}.location"), Yaml::String(auction::write_location(&here))));
            });
        });
    }
    {
        let (n, base) = (n.clone(), base.clone());
        let mut lore = vec![" ".to_owned(), current("")];
        lore.extend(lines.iter().map(|l| format!(" &f{l}")));
        lore.extend(multi_lore());
        m.button(34, icon("PLAYER_HEAD", "&#D200FF&lNPC Name", &lore), move |c| {
            let key = format!("{base}.name");
            multi_text(&c.player, c.click, lines.clone(), {
                let n = n.clone();
                move |p| npc(p, &n, route)
            }, {
                let n = n.clone();
                move |list| edit(&n, |cfg| cfg.set(&key, strings(&list)))
            });
        });
    }
    {
        let n = n.clone();
        m.button(49, icon("BARRIER", "&#D200FF&lBack", &[]), move |c| main(&c.player, &n));
    }
    m.open(p);
}

fn multi_lore() -> [String; 4] {
    [" ".into(), "&#D200FF&l> &#D200FFLeft Click &8- &#00FF00Add New Line".into(), "&#D200FF&l> &#D200FFRight Click &8- &#FF0000Delete Last Line".into(), "&#D200FF&l> &#D200FFShift + Right Click &8- &#DD0000Clear All Lines".into()]
}

fn strings(list: &[String]) -> Yaml {
    Yaml::Sequence(list.iter().map(|s| Yaml::String(s.clone())).collect())
}

/// addInputMultiText: left adds a line (typed in chat), right drops the last, shift-right clears all.
fn multi_text(p: &Player, click: Click, mut list: Vec<String>, back: impl Fn(&Player) + Send + Clone + 'static, save: impl Fn(Vec<String>) + Send + 'static) {
    match click {
        Click::ShiftRight => {
            save(Vec::new());
            back(p);
        }
        Click::Right => {
            list.pop();
            save(list);
            back(p);
        }
        _ => {
            ask_text(p, "&#FF00FFWrite the new text: &#DDDDDD(write &#FF00FFcancel &#DDDDDDto stop)", back, move |_, line| {
                list.push(line);
                save(list);
            });
        }
    }
}

// ---- RewardEditor ----

fn items_of(name: &str) -> Vec<Yaml> {
    auction::with(name, |a| match a.cfg.at("items") {
        Some(Yaml::Sequence(s)) => s.clone(),
        _ => Vec::new(),
    })
    .unwrap_or_default()
}

fn set_items(name: &str, items: Vec<Yaml>) {
    edit(name, |cfg| cfg.set("items", Yaml::Sequence(items)));
}

fn price_of(entry: &Yaml) -> (String, f64) {
    let price = entry.get("starting-price");
    let currency = price.and_then(|p| horizon_kit::yaml::string(p, "currency")).unwrap_or_else(|| "Vault".into());
    let amount = price.map_or(100.0, |p| horizon_kit::yaml::num(p, "amount", 100.0));
    (currency, amount)
}

pub fn rewards(p: &Player, name: &str, page: usize) {
    let items = items_of(name);
    let mut m = Menu::new(6, &title(name));
    frame(&mut m);
    let content: Vec<usize> = (9..45).collect();
    let pages = items.len().div_ceil(content.len()).max(1);
    let page = page.min(pages - 1);
    let n = name.to_owned();
    for (slot, (index, entry)) in content.iter().zip(items.iter().enumerate().skip(page * content.len())) {
        let mut stack = crate::menus::reward_item(entry.get("display").unwrap_or(&Yaml::Null)).unwrap_or_else(|| gui::filler("PAPER"));
        let (currency, amount) = price_of(entry);
        let chance = horizon_kit::yaml::num(entry, "chance", 50.0);
        let price = if crate::currency(&currency).is_some() { crate::format_money(&currency, amount) } else { "---".into() };
        let mut lore = stack.get_lore().iter().map(|l| l.replace('§', "&")).collect::<Vec<_>>();
        lore.extend([" ".into(), format!("&#D200FF&l> &#FF00FFChance: &#D200FF{chance}%"), format!("&#D200FF&l> &#FF00FFStarting Price: &#D200FF{price}"), " ".into(), "&#D200FF&l> &#D200FFLeft Click &8- &#D200FFEdit Reward".into(), "&#D200FF&l> &#D200FFDrop (Q) &8- &#D200FFDelete Reward".into()]);
        horizon_kit::item::lore(&mut stack, &lore);
        let n = n.clone();
        m.button(*slot, stack, move |c| {
            if c.click == Click::Drop {
                let mut all = items_of(&n);
                if index < all.len() {
                    all.remove(index);
                    set_items(&n, all);
                }
                rewards(&c.player, &n, page);
            } else {
                reward(&c.player, &n, index);
            }
        });
    }
    {
        let n = n.clone();
        let lore = vec![" ".into(), "&#D200FF&l> &#D200FFLeft Click While Holding Item &8- &#D200FFAdd New Item Reward".into(), "&#D200FF&l> &#D200FFRight Click While Holding Item &8- &#D200FFAdd New Command Reward".into()];
        m.button(4, icon("GOLD_INGOT", "&#D200FF&lAdd Reward", &lore), move |c| {
            let Some(stack) = held(&c.player) else {
                return crate::tell_lang(&c.player, "errors.hold-something", &[]);
            };
            let display = serialize(&stack);
            let mut entry = Mapping::new();
            entry.insert("chance".into(), Yaml::Number(10.0.into()));
            entry.insert("display".into(), display.clone());
            if c.click.is_right() {
                let n2 = n.clone();
                let n3 = n.clone();
                ask_text(&c.player, "&#FF4400Write the command here without the /slash: &#DDDDDD(write &#FF4400cancel &#DDDDDDto stop)\n&#DDDDDD(use %player% as a placeholder for the player)", move |p| rewards(p, &n2, page), move |_, line| {
                    entry.insert("commands".into(), strings(&[line]));
                    let mut all = items_of(&n3);
                    all.push(Yaml::Mapping(entry));
                    set_items(&n3, all);
                });
            } else {
                entry.insert("items".into(), Yaml::Sequence(vec![display]));
                entry.insert("commands".into(), Yaml::Sequence(Vec::new()));
                let mut all = items_of(&n);
                all.push(Yaml::Mapping(entry));
                set_items(&n, all);
                rewards(&c.player, &n, page);
            }
        });
    }
    if page > 0 {
        let n = n.clone();
        m.button(47, icon("ARROW", "&#D200FF&lPrevious", &[]), move |c| rewards(&c.player, &n, page - 1));
    }
    if page + 1 < pages {
        let n = n.clone();
        m.button(51, icon("ARROW", "&#D200FF&lNext", &[]), move |c| rewards(&c.player, &n, page + 1));
    }
    {
        let n = n.clone();
        m.button(49, icon("BARRIER", "&#D200FF&lBack", &[]), move |c| main(&c.player, &n));
    }
    m.open(p);
}

// ---- RewardSettingEditor ----

fn change_reward(name: &str, index: usize, change: impl FnOnce(&mut Mapping)) {
    let mut all = items_of(name);
    if let Some(Yaml::Mapping(entry)) = all.get_mut(index) {
        change(entry);
        set_items(name, all);
    }
}

fn price_map(entry: &mut Mapping) -> &mut Mapping {
    let key = Yaml::String("starting-price".into());
    if !entry.get(&key).is_some_and(Yaml::is_mapping) {
        entry.insert(key.clone(), Yaml::Mapping(Mapping::new()));
    }
    entry.get_mut(&key).and_then(Yaml::as_mapping_mut).expect("a mapping")
}

pub fn reward(p: &Player, name: &str, index: usize) {
    let all = items_of(name);
    let Some(entry) = all.get(index).cloned() else { return rewards(p, name, 0) };
    let chance = horizon_kit::yaml::num(&entry, "chance", 50.0);
    let (currency, amount) = price_of(&entry);
    let commands = horizon_kit::yaml::list(&entry, "commands");
    let mut m = Menu::new(6, &title(name));
    frame(&mut m);
    let n = name.to_owned();
    let back = {
        let n = n.clone();
        move |p: &Player| reward(p, &n, index)
    };
    {
        let (n, back) = (n.clone(), back.clone());
        let lore = vec![" ".into(), current(&format!("{chance}%")), " ".into(), "&#D200FF&l> &#D200FFClick &8- &#D200FFModify".into()];
        m.button(20, icon("DIAMOND", "&#D200FF&lChance", &lore), move |c| {
            let n = n.clone();
            ask_text(&c.player, "&#FF00FFWrite the new text: &#DDDDDD(write &#FF00FFcancel &#DDDDDDto stop)", back.clone(), move |_, line| {
                if let Ok(v) = line.trim().parse::<f64>() {
                    change_reward(&n, index, |e| {
                        e.insert("chance".into(), Yaml::Number(v.into()));
                    });
                }
            });
        });
    }
    {
        let n = n.clone();
        m.button(22, icon("GOLD_INGOT", "&#D200FF&lItem Rewards", &[" ".into(), "&#D200FF&l> &#D200FFClick &8- &#D200FFOpen Menu".into()]), move |c| reward_items(&c.player, &n, index));
    }
    {
        let (n, back) = (n.clone(), back.clone());
        let mut lore = vec![String::new(), "&#D200FF&l> &#FF00FFCurrent commands:".into()];
        lore.extend(commands.iter().map(|c| format!("&#DDDDDD/{c}")));
        lore.extend(multi_lore());
        m.button(24, icon("COMMAND_BLOCK", "&#D200FF&lCommand Rewards", &lore), move |c| {
            let n = n.clone();
            multi_text(&c.player, c.click, commands.clone(), back.clone(), move |list| {
                change_reward(&n, index, |e| {
                    e.insert("commands".into(), strings(&list));
                })
            });
        });
    }
    {
        let (n, back) = (n.clone(), back.clone());
        let names = crate::currency_names();
        let lore = vec!["&#DDDDDDUsed for the starting price".into(), " ".into(), current(&currency), " ".into(), "&#D200FF&l> &#D200FFLeft Click &8- &#D200FFNext Value".into(), "&#D200FF&l> &#D200FFRight Click &8- &#D200FFPrevious Value".into()];
        m.button(30, icon("EMERALD_BLOCK", "&#D200FF&lCurrency", &lore), move |c| {
            if names.is_empty() {
                return;
            }
            let at = names.iter().position(|x| x.eq_ignore_ascii_case(&currency)).unwrap_or(0);
            let next = if c.click.is_left() { (at + 1) % names.len() } else { (at + names.len() - 1) % names.len() };
            let chosen = names[next].clone();
            change_reward(&n, index, |e| {
                price_map(e).insert("currency".into(), Yaml::String(chosen));
            });
            back(&c.player);
        });
    }
    {
        let (n, back) = (n.clone(), back.clone());
        let lore = vec![" ".into(), current(&amount.to_string()), " ".into(), "&#D200FF&l> &#D200FFClick &8- &#D200FFModify".into()];
        m.button(32, icon("EMERALD", "&#D200FF&lStarting Price", &lore), move |c| {
            let n = n.clone();
            ask_text(&c.player, "&#FF00FFWrite the new text: &#DDDDDD(write &#FF00FFcancel &#DDDDDDto stop)", back.clone(), move |_, line| {
                if let Ok(v) = line.trim().parse::<f64>() {
                    change_reward(&n, index, |e| {
                        price_map(e).insert("amount".into(), Yaml::Number(v.into()));
                    });
                }
            });
        });
    }
    {
        let n = n.clone();
        m.button(49, icon("BARRIER", "&#D200FF&lBack", &[]), move |c| rewards(&c.player, &n, 0));
    }
    m.open(p);
}

// ---- RewardItemsEditor ----

/// The items a winner gets, as a chest to put them in: rows 2-5 are the items, read back when it closes.
pub fn reward_items(p: &Player, name: &str, index: usize) {
    let all = items_of(name);
    let Some(entry) = all.get(index).cloned() else { return };
    let mut m = Menu::new(6, &title(name));
    m.allow_take(true);
    // The frame is buttons that do nothing: nothing can be taken from it or put in it.
    let pane = gui::filler(FRAME);
    for s in (0..9).chain(45..54) {
        m.button(s, pane.clone(), |_| {});
    }
    let items: Vec<Yaml> = match entry.get("items") {
        Some(Yaml::Sequence(s)) => s.clone(),
        _ => Vec::new(),
    };
    for (slot, item) in (9..45).zip(items.iter()) {
        if let Some(stack) = crate::menus::reward_item(item) {
            m.item(slot, stack);
        }
    }
    let n = name.to_owned();
    {
        let n = n.clone();
        m.button(49, icon("BARRIER", "&#D200FF&lBack", &[]), move |c| reward(&c.player, &n, index));
    }
    let inventory = m.inventory().clone();
    m.on_close(move |player| {
        let live = inventory.live();
        let kept: Vec<Yaml> = (9..45).filter_map(|s| live.get_item(s)).filter(|s| !s.is_empty()).map(serialize).collect();
        let n2 = n.clone();
        change_reward(&n, index, |e| {
            e.insert("items".into(), Yaml::Sequence(kept));
        });
        let who = player.clone();
        horizon_kit::app::later(1, move || {
            if horizon_kit::gui::open_tag(&who).is_none() {
                reward(&who, &n2, index);
            }
            crate::finish();
        });
    });
    m.open(p);
}

// ---- SchedulerEditor ----

pub fn schedulers(p: &Player) {
    schedulers_page(p, 0);
}

fn schedulers_page(p: &Player, page: usize) {
    let names: Vec<String> = crate::settings(|s| s.schedules.all.iter().map(|x| x.name.clone()).collect()).unwrap_or_default();
    let mut m = Menu::new(5, &crate::texts::legacy("&0Editing schedulers"));
    let per = 36;
    let pages = names.len().div_ceil(per).max(1);
    let page = page.min(pages - 1);
    for (slot, name) in (0..per).zip(names.iter().skip(page * per)) {
        let next = auction::next_scheduler_in(name);
        let shown = next.map_or_else(|| "Not scheduled".to_owned(), crate::format_time);
        let lore = vec![format!("&#DDDDDDNext run: &f{shown}"), String::new(), "&#D200FFLeft Click &7- &#00FF00Edit Schedule".into(), "&#D200FFRight Click &7- &#FF0000Remove Schedule".into()];
        let name = name.clone();
        m.button(slot, icon("CLOCK", &format!("&#D200FF&l{name}"), &lore), move |c| {
            if c.click.is_right() {
                crate::save_schedulers(|cfg| {
                    if let Some(map) = cfg.doc.as_mapping_mut() {
                        map.remove(Yaml::String(name.clone()));
                    }
                });
                crate::texts::send(&c.player, "&#D200FFSuccessfully removed schedule!");
                schedulers_page(&c.player, page);
            } else {
                scheduler(&c.player, &name);
            }
        });
    }
    m.button(40, icon("CLOCK", "&#D200FF&lClick here to create a new schedule!", &[]), move |c| {
        ask_text(&c.player, "&#D200FFWrite the name of the new schedule: &#DDDDDD(write &#D200FFcancel &#DDDDDDto stop)", move |p| schedulers_page(p, page), |p, line| {
            let name = line.trim().replace(' ', "-");
            if name.is_empty() {
                return;
            }
            crate::save_schedulers(|cfg| {
                cfg.set(&format!("{name}.times"), Yaml::Sequence(Vec::new()));
                cfg.set(&format!("{name}.random"), Yaml::Bool(false));
                cfg.set(&format!("{name}.auctions"), Yaml::Sequence(Vec::new()));
            });
            crate::texts::send(p, "&#D200FFAdded new schedule! Make sure to open it and set it up, otherwise it won't work!");
        });
    });
    if page > 0 {
        m.button(38, icon("ARROW", "&#D200FF&lPrevious", &[]), move |c| schedulers_page(&c.player, page - 1));
    }
    if page + 1 < pages {
        m.button(42, icon("ARROW", "&#D200FF&lNext", &[]), move |c| schedulers_page(&c.player, page + 1));
    }
    m.open(p);
}

// ---- Scheduler2Editor ----

pub fn scheduler(p: &Player, name: &str) {
    // Auctions that don't exist and times that don't parse are taken out first, each one said.
    let (auctions, times, random) = crate::settings(|s| (s.schedulers_file.list(&format!("{name}.auctions")), s.schedulers_file.list(&format!("{name}.times")), s.schedulers_file.bool(&format!("{name}.random"), false))).unwrap_or_default();
    let known: Vec<String> = auctions.iter().filter(|a| {
        let ok = auction::exists(a);
        if !ok {
            crate::texts::send(p, &format!("&#7700FFAuction not found: &#D200FF{a} &#7700FFThe plugin has removed it to prevent errors!"));
        }
        ok
    }).cloned().collect();
    let valid: Vec<String> = times.iter().filter(|t| {
        let ok = crate::schedule::Cron::parse(t).is_ok();
        if !ok {
            crate::texts::send(p, &format!("&#7700FFInvalid CRON format: &#D200FF{t} &#7700FFThe plugin has removed it to prevent errors!"));
        }
        ok
    }).cloned().collect();
    if known.len() != auctions.len() || valid.len() != times.len() {
        let (k, v) = (known.clone(), valid.clone());
        crate::save_schedulers(|cfg| {
            cfg.set(&format!("{name}.auctions"), strings(&k));
            cfg.set(&format!("{name}.times"), strings(&v));
        });
    }
    let mut m = Menu::new(5, &crate::texts::legacy(&format!("&0Editing schedule: &l{name}")));
    let n = name.to_owned();
    let back = {
        let n = n.clone();
        move |p: &Player| scheduler(p, &n)
    };
    {
        let (n, back) = (n.clone(), back.clone());
        let mut lore = vec!["&#DDDDDDThese auctions will be started when the schedule runs.".to_owned(), "&#DDDDDDIf 'random' is enabled, only one of these will be started.".into(), " ".into(), "&#7700FF&l> &#D200FFCurrently selected:".into()];
        lore.extend(known.iter().map(|a| format!("&f{a}")));
        lore.extend(multi_lore());
        m.button(20, icon("OAK_SIGN", "&#7700FF&lSelected auctions", &lore), move |c| {
            let n = n.clone();
            let key = format!("{n}.auctions");
            match c.click {
                Click::ShiftRight | Click::Right => multi_text(&c.player, c.click, known.clone(), back.clone(), move |list| crate::save_schedulers(|cfg| cfg.set(&key, strings(&list)))),
                _ => {
                    let list = known.clone();
                    let message = format!("&#7700FFWrite the name of the dark auction here: &#DDDDDD(write &#7700FFcancel &#DDDDDDto stop)\n&#DDDDDDList of auction: &f{}", auction::names().join(", "));
                    ask_text(&c.player, &message, back.clone(), move |_, line| {
                        let mut list = list;
                        list.push(line.trim().to_owned());
                        crate::save_schedulers(|cfg| cfg.set(&key, strings(&list)));
                    });
                }
            }
        });
    }
    {
        let (n, back) = (n.clone(), back.clone());
        let lore = vec!["&#DDDDDDIf enabled, a random dark auction will be started".to_owned(), "&#DDDDDDfrom the list of selected auctions.".into(), " ".into(), format!("&#7700FF&l> &#D200FFCurrent value: &f{random}"), " ".into(), "&#D200FF&l> &#D200FFClick &8- &#D200FFToggle".into()];
        m.button(13, icon(if random { "LIME_DYE" } else { "GRAY_DYE" }, "&#7700FF&lRandom", &lore), move |c| {
            crate::save_schedulers(|cfg| cfg.set(&format!("{n}.random"), Yaml::Bool(!random)));
            back(&c.player);
        });
    }
    {
        let (n, back) = (n.clone(), back.clone());
        let mut lore = vec!["&#DDDDDDThe times when this schedule will run.".to_owned(), "&#DDDDDDThis uses the cron format, there are".into(), "&#DDDDDDsome generators online if you need help!".into(), " ".into(), "&#7700FF&l> &#D200FFCurrent schedules:".into()];
        lore.extend(valid.iter().map(|t| format!("&f{t}")));
        lore.extend([" ".into(), "&#7700FFFormat:".into(), "&f[MINUTE] [HOUR] [DAY] [MONTH] [DAY OF WEEK]".into(), "&#7700FFYou use a number or these:".into(), " &f* &#D200FFany value &#DDDDDD(ex: 1)".into(), " &f, &#D200FFmultiple separated values &#DDDDDD(ex: 1,2,3)".into(), " &f- &#D200FFrange of values &#DDDDDD(ex: 15-22)".into(), " &f/ &#D200FFstep values &#DDDDDD(ex: 0/2)".into()]);
        lore.extend(multi_lore());
        m.button(24, icon("CLOCK", "&#7700FF&lScheduler Run Times", &lore), move |c| {
            let key = format!("{n}.times");
            match c.click {
                Click::ShiftRight | Click::Right => multi_text(&c.player, c.click, valid.clone(), back.clone(), move |list| crate::save_schedulers(|cfg| cfg.set(&key, strings(&list)))),
                _ => {
                    let list = valid.clone();
                    ask_text(&c.player, "&#7700FFWrite the new schedule time here: &#DDDDDD(write &#7700FFcancel &#DDDDDDto stop)\n&#DDDDDDIf you need help, you can use an online generator like https://crontab.guru/", back.clone(), move |p, line| {
                        if let Err(e) = crate::schedule::Cron::parse(line.trim()) {
                            crate::texts::send(p, &format!("&#7700FFInvalid CRON format: &#D200FF{line} &#7700FF({e})"));
                            return;
                        }
                        let mut list = list;
                        list.push(line.trim().to_owned());
                        crate::save_schedulers(|cfg| cfg.set(&key, strings(&list)));
                    });
                }
            }
        });
    }
    m.button(40, icon("BARRIER", "&#7700FF&lBack", &[]), |c| schedulers(&c.player));
    m.open(p);
}

#[cfg(test)]
mod tests {
    #[test]
    fn steps() {
        assert_eq!(super::step(super::Click::ShiftLeft), 10);
        assert_eq!(super::step(super::Click::Right), -1);
    }
}
