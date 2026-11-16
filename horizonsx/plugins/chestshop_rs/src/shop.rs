//! Shop signs and their containers (`ChestShopSign`, `uBlock`, `PriceUtil`): reading and checking the four lines,
//! the container a sign belongs to (the block it hangs on first, then the six around it), the sign of a
//! container, double chests as one inventory.

use horizon_bukkit::fam_items::{BlockInventory, DoubleChest};
use horizon_bukkit::{Block, ItemStack, Player};
use regex::Regex;
use std::sync::LazyLock;

static QUANTITY: LazyLock<[Regex; 2]> = LazyLock::new(|| [Regex::new(r"^[1-9][0-9]{0,5}$").unwrap(), Regex::new(r"^Q [1-9][0-9]{0,4} : C [0-9]{1,5}$").unwrap()]);
static PRICE: LazyLock<[Regex; 5]> = LazyLock::new(|| {
    [
        Regex::new(r"(?i)^((\d*([.e]\d+)?)|free)$").unwrap(),
        Regex::new(r"(?i)^([BS] *((\d*([.e]\d+)?)|free))( *: *([BS] *((\d*([.e]\d+)?)|free)))?$").unwrap(),
        Regex::new(r"(?i)^(((\d*([.e]\d+)?)|free) *[BS])( *: *([BS] *((\d*([.e]\d+)?)|free)))?$").unwrap(),
        Regex::new(r"(?i)^(((\d*([.e]\d+)?)|free) *[BS]) *: *(((\d*([.e]\d+)?)|free) *[BS])$").unwrap(),
        Regex::new(r"(?i)^([BS] *((\d*([.e]\d+)?)|free)) *: *(((\d*([.e]\d+)?)|free) *[BS])$").unwrap(),
    ]
});
static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\p{L}\d_? #:\-]+$").unwrap());
static NAME_WITH_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.+):[A-Za-z0-9]+$").unwrap());

/// `ChatColor.stripColor`.
pub fn strip_colour(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// The four lines, colours off, trimmed (`StringUtil.strip`).
pub fn clean(lines: &[String]) -> [String; 4] {
    let get = |i: usize| strip_colour(lines.get(i).map_or("", String::as_str)).trim().to_owned();
    [get(0), get(1), get(2), get(3)]
}

pub fn is_admin_name(owner: &str, admin_name: &str) -> bool {
    owner.replace(' ', "").eq_ignore_ascii_case(&admin_name.replace(' ', ""))
}

/// `isValidPreparedSign`: the lines look like a shop (before the plugin fills them in).
pub fn valid_prepared(l: &[String; 4], admin_name: &str, name_re: Option<&Regex>) -> bool {
    let mut owner = l[0].clone();
    if !is_admin_name(&owner, admin_name) && !owner.is_empty() {
        if let Some(c) = NAME_WITH_ID.captures(&owner) {
            owner = c[1].to_owned();
        }
        if name_re.is_some_and(|re| !re.is_match(&owner)) {
            return false;
        }
    }
    let ok = QUANTITY.iter().any(|r| r.is_match(&l[1])) && PRICE.iter().any(|r| r.is_match(&l[2])) && ITEM.is_match(&l[3]);
    ok && l[2].find(':') == l[2].rfind(':')
}

/// `isValid`: a finished shop sign.
pub fn valid(l: &[String; 4], admin_name: &str, name_re: Option<&Regex>) -> bool {
    let up = l[2].to_ascii_uppercase();
    valid_prepared(l, admin_name, name_re) && (up.contains('B') || up.contains('S')) && !l[0].is_empty()
}

/// `QuantityUtil.parseQuantity`.
pub fn quantity(line: &str) -> Option<i64> {
    if QUANTITY[1].is_match(line) {
        return line.split(" : ").next()?.trim_start_matches("Q ").parse().ok();
    }
    line.parse().ok()
}

/// `PriceUtil.getExact`: the buy (`b`) or sell (`s`) price; `None` = this shop doesn't.
pub fn price(text: &str, indicator: char) -> Option<f64> {
    let t = text.replace(' ', "").to_lowercase();
    for part in t.split(':') {
        if part.starts_with(indicator) || part.ends_with(indicator) {
            let p = part.replace(indicator, "");
            if p == "free" {
                return Some(0.0);
            }
            if let Ok(v) = p.parse::<f64>() {
                return (v >= 0.0 && v.is_finite()).then_some(v);
            }
        }
    }
    None
}

pub fn is_price(text: &str) -> bool {
    text.trim().parse::<f64>().is_ok() || text.trim().eq_ignore_ascii_case("free")
}

// ---- Blocks ----

pub fn is_sign_type(key: &str) -> bool {
    key.ends_with("_sign")
}

/// The face a wall sign hangs on (the block behind it), from its block data.
fn wall_offset(data: &str) -> Option<(i32, i32, i32)> {
    if !data.contains("wall_") || !data.contains("sign") {
        return None;
    }
    let facing = data.split(['[', ',', ']']).find_map(|p| p.strip_prefix("facing="))?;
    Some(match facing {
        "north" => (0, 0, 1),
        "south" => (0, 0, -1),
        "east" => (-1, 0, 0),
        "west" => (1, 0, 0),
        _ => return None,
    })
}

/// `SHOP_FACES`: self, down, up, east, north, west, south.
const FACES: [(i32, i32, i32); 7] = [(0, 0, 0), (0, -1, 0), (0, 1, 0), (1, 0, 0), (0, 0, -1), (-1, 0, 0), (0, 0, 1)];

pub fn is_container(block: &Block, containers: &[String]) -> bool {
    containers.iter().any(|c| *c == block.get_type().name())
}

/// `findConnectedContainer(sign)`: the block it hangs on, else the first container around it.
pub fn container_of(sign: &Block, containers: &[String]) -> Option<Block> {
    let behind = wall_offset(&sign.get_block_data());
    if let Some((x, y, z)) = behind {
        let b = sign.get_relative(x, y, z);
        if is_container(&b, containers) {
            return Some(b);
        }
    }
    for f in FACES {
        if Some(f) == behind {
            continue;
        }
        let b = sign.get_relative(f.0, f.1, f.2);
        if is_container(&b, containers) {
            return Some(b);
        }
    }
    None
}

/// A sign's lines (the front).
pub fn lines(block: &Block) -> Vec<String> {
    horizon_bukkit::blocks::Sign(block.tile()).get_lines()
}

/// `findAnyNearbyShopSign`: a valid shop sign next to `block` that belongs to it.
fn nearby_sign(block: &Block, containers: &[String], admin: &str, re: Option<&Regex>) -> Option<(Block, [String; 4])> {
    for f in FACES {
        let b = block.get_relative(f.0, f.1, f.2);
        if !is_sign_type(b.get_type().key()) {
            continue;
        }
        let data = b.get_block_data();
        if let Some((x, y, z)) = wall_offset(&data) {
            // Hangs on `block`, or on something that is no shop container.
            let attached = (x, y, z) == (-f.0, -f.1, -f.2);
            if !attached && is_container(&b.get_relative(x, y, z), containers) {
                continue;
            }
        }
        let l = clean(&lines(&b));
        if valid(&l, admin, re) {
            return Some((b, l));
        }
    }
    None
}

/// `getConnectedSign`: the shop sign of a container (either half of a double chest).
pub fn sign_of(container: &Block, containers: &[String], admin: &str, re: Option<&Regex>) -> Option<(Block, [String; 4])> {
    if let Some(s) = nearby_sign(container, containers, admin, re) {
        return Some(s);
    }
    let other = DoubleChest::of(container).map(|d| {
        let l = d.get_left_side().get_block().clone();
        if l == *container { d.get_right_side().get_block().clone() } else { l }
    })?;
    nearby_sign(&other, containers, admin, re)
}

pub fn key(b: &Block) -> String {
    format!("{};{};{};{}", b.get_world().get_name(), b.get_x(), b.get_y(), b.get_z())
}

/// A shop's container as one inventory (both halves of a double chest).
pub struct ShopInv {
    halves: Vec<BlockInventory>,
}

impl ShopInv {
    pub fn of(container: &Block) -> ShopInv {
        match DoubleChest::of(container) {
            Some(d) => ShopInv { halves: vec![d.get_left_side(), d.get_right_side()] },
            None => ShopInv { halves: vec![BlockInventory::of(container)] },
        }
    }
    pub fn contents(&self) -> Vec<Option<ItemStack>> {
        self.halves.iter().flat_map(BlockInventory::get_contents).collect()
    }
    pub fn count(&self, like: &ItemStack) -> i64 {
        count_in(&self.contents(), like)
    }
    pub fn room_for(&self, like: &ItemStack) -> i64 {
        room_in(&self.contents(), like)
    }
    /// What didn't fit comes back.
    pub fn add(&self, stack: &ItemStack) -> i64 {
        let mut left = i64::from(stack.get_amount());
        for h in &self.halves {
            if left <= 0 {
                break;
            }
            for chunk in chunks(stack, left) {
                let rest = h.add_item(&chunk).map_or(0, |r| i64::from(r.get_amount()));
                left -= i64::from(chunk.get_amount()) - rest;
            }
        }
        left
    }
    /// What couldn't be found comes back.
    pub fn remove(&self, stack: &ItemStack) -> i64 {
        let mut left = i64::from(stack.get_amount());
        for h in &self.halves {
            if left <= 0 {
                break;
            }
            let rest = h.remove_item(&stack.as_quantity(left as i32)).map_or(0, |r| i64::from(r.get_amount()));
            left = rest;
        }
        left
    }
}

/// A stack split into stacks the game holds (no more than its max size each).
pub fn chunks(stack: &ItemStack, amount: i64) -> Vec<ItemStack> {
    let max = i64::from(stack.get_max_stack_size().max(1));
    let mut out = Vec::new();
    let mut left = amount;
    while left > 0 {
        let n = left.min(max);
        out.push(stack.as_quantity(n as i32));
        left -= n;
    }
    out
}

pub fn count_in(contents: &[Option<ItemStack>], like: &ItemStack) -> i64 {
    contents.iter().flatten().filter(|s| crate::items::same(s, like)).map(|s| i64::from(s.get_amount())).sum()
}

pub fn room_in(contents: &[Option<ItemStack>], like: &ItemStack) -> i64 {
    let max = i64::from(like.get_max_stack_size().max(1));
    contents
        .iter()
        .map(|s| match s {
            None => max,
            Some(s) if crate::items::same(s, like) => (max - i64::from(s.get_amount())).max(0),
            Some(_) => 0,
        })
        .sum()
}

/// A player's storage slots (hotbar and the three rows).
pub fn storage(p: &Player) -> Vec<Option<ItemStack>> {
    let mut c = p.get_inventory().get_contents();
    c.resize(36, None);
    c.truncate(36);
    c
}

/// Takes `amount` of `like` from a player's storage; what couldn't be taken comes back.
pub fn take_from_player(p: &Player, like: &ItemStack, amount: i64) -> i64 {
    let inv = p.get_inventory();
    let mut left = amount;
    for (slot, s) in storage(p).into_iter().enumerate() {
        if left <= 0 {
            break;
        }
        let Some(s) = s.filter(|s| crate::items::same(s, like)) else { continue };
        let have = i64::from(s.get_amount());
        if have <= left {
            inv.clear_slot(slot as i32);
            left -= have;
        } else {
            inv.set_item(slot as i32, &s.as_quantity((have - left) as i32));
            left = 0;
        }
    }
    left
}

/// Gives `amount` of `like`; what didn't fit comes back.
pub fn give_to_player(p: &Player, like: &ItemStack, amount: i64) -> i64 {
    let inv = p.get_inventory();
    let mut left = 0;
    for chunk in chunks(like, amount) {
        left += inv.add_item(&chunk).map_or(0, |r| i64::from(r.get_amount()));
    }
    left
}

#[cfg(test)]
mod tests {
    #[test]
    fn prices() {
        assert_eq!(super::price("B 10:5 S", 'b'), Some(10.0));
        assert_eq!(super::price("B 10:5 S", 's'), Some(5.0));
        assert_eq!(super::price("B free", 'b'), Some(0.0));
        assert_eq!(super::price("B 10", 's'), None);
        let l = ["Notch".to_owned(), "64".to_owned(), "B 10:5 S".to_owned(), "Diamond".to_owned()];
        assert!(super::valid(&l, "Admin Shop", None));
        assert_eq!(super::quantity("Q 64 : C 10"), Some(64));
    }
}
