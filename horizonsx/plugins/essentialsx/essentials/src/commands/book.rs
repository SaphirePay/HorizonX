//! `Commandbook`.
use crate::prelude::*;
use horizon_bukkit::meta::{BookMeta, WritableBookMeta};

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("book") };

fn is_author(meta: &BookMeta, player: &str) -> bool {
    meta.get_author().is_some_and(|author| author.eq_ignore_ascii_case(player))
}

fn pages_of(list: &[String]) -> Vec<&str> {
    list.iter().map(String::as_str).collect()
}

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let Some(base) = user.player() else { return Ok(()) };
    let item = inventories::get_item_in_main_hand(&base).ok_or_else(|| EssError::tl("holdBook", &[]))?;
    let player = user.get_name();
    let word = args.first().filter(|_| args.len() > 1).map(|first| first.to_lowercase());
    match item.get_type().name().as_str() {
        "WRITTEN_BOOK" => {
            let mut meta = BookMeta(item.clone());
            let mine = is_author(&meta, &player) || user.is_authorized("essentials.book.others");
            match word.as_deref() {
                Some("author") => {
                    fail_if!(!user.is_authorized("essentials.book.author") || !mine, EssError::tl("denyChangeAuthor", &[]));
                    let author = format_string(user, "essentials.book.author", &get_final_arg(args, 1)).trim().to_owned();
                    meta.set_author(&author);
                    inventories::set_item_in_main_hand(&base, &meta.0);
                    user.send_tl("bookAuthorSet", crate::args![author]);
                }
                Some("title") => {
                    fail_if!(!user.is_authorized("essentials.book.title") || !mine, EssError::tl("denyChangeTitle", &[]));
                    let title = format_string(user, "essentials.book.title", &get_final_arg(args, 1)).trim().to_owned();
                    meta.set_title(&title);
                    inventories::set_item_in_main_hand(&base, &meta.0);
                    user.send_tl("bookTitleSet", crate::args![title]);
                }
                _ => {
                    fail_if!(!mine, EssError::tl("denyBookEdit", &[]));
                    let pages = meta.get_pages();
                    let mut writable = WritableBookMeta(ItemStack::new(Material::of("WRITABLE_BOOK"), item.get_amount()));
                    writable.set_pages(&pages_of(&pages));
                    inventories::set_item_in_main_hand(&base, &writable.0);
                    user.send_tl("editBookContents", &[]);
                }
            }
        }
        "WRITABLE_BOOK" => {
            let pages = WritableBookMeta(item.clone()).get_pages();
            let mut real = BookMeta(ItemStack::new(Material::of("WRITTEN_BOOK"), item.get_amount()));
            real.set_author(if user.is_authorized("essentials.book.author") { "" } else { &player });
            real.set_title("");
            real.set_pages(&pages_of(&pages));
            inventories::set_item_in_main_hand(&base, &real.0);
            user.send_tl("bookLocked", &[]);
        }
        _ => return Err(EssError::tl("holdBook", &[])),
    }
    Ok(())
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => {
            let mut options = vec!["sign".to_owned(), "unsign".to_owned()];
            if user.is_authorized("essentials.book.author") {
                options.push("author".to_owned());
            }
            if user.is_authorized("essentials.book.title") {
                options.push("title".to_owned());
            }
            options
        }
        2 if args[0].eq_ignore_ascii_case("author") && user.is_authorized("essentials.book.author") => {
            let mut options = get_players(&user.get_source());
            options.push("Herobrine".to_owned());
            options
        }
        _ => Vec::new(),
    })
}
