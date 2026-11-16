//! The currencies a starting price and a bid are in (currencies.yml). On Horizon: `Vault` (the server's economy,
//! Essentials' through the Vault service), `Experience` (levels and progress, as the Java hook counts them) and
//! the `placeholder-currencies` (a balance placeholder plus give/take commands). The other plugins' hooks of the
//! Java version (PlayerPoints, CoinsEngine, ...) only register when such a plugin's economy service is there.

use horizon_bukkit::Player;
use horizon_kit::{hooks, yaml};
use serde_yaml::Value as Yaml;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Vault,
    Experience,
    Placeholder { raw: String, give: String, take: String, uses_double: bool, offline: bool },
}

#[derive(Clone, Debug)]
pub struct Currency {
    pub name: String,
    pub kind: Kind,
    /// `name: "&f%price% &#AAFFFFcoins"`: how an amount of it reads.
    pub display: String,
}

impl Currency {
    pub fn works_offline(&self) -> bool {
        match &self.kind {
            Kind::Vault => true,
            Kind::Experience => false,
            Kind::Placeholder { offline, .. } => *offline,
        }
    }

    pub fn balance(&self, player: &Player) -> f64 {
        match &self.kind {
            Kind::Vault => hooks::economy::balance(player.get_name()),
            Kind::Experience => f64::from(total_exp(player.get_level(), player.get_exp())),
            Kind::Placeholder { raw, .. } => hooks::placeholders(raw, Some(player)).trim().replace(',', "").parse().unwrap_or(0.0),
        }
    }

    /// Takes the amount (the winner pays): true when it went through.
    pub fn take(&self, player: &Player, amount: f64) -> bool {
        match &self.kind {
            Kind::Vault => hooks::economy::withdraw(player.get_name(), amount),
            Kind::Experience => {
                change_exp(player, -(amount as i32));
                true
            }
            Kind::Placeholder { take, uses_double, .. } => run(take, player, amount, *uses_double),
        }
    }
}

fn run(command: &str, player: &Player, amount: f64, uses_double: bool) -> bool {
    let shown = if uses_double { crate::texts::decimal(amount, "#") } else { format!("{}", amount.round() as i64) };
    horizon_bukkit::dispatch_command(&command.replace("%amount%", &shown).replace("%player%", player.get_name()))
}

/// The Java hook's experience maths: points for a level, plus the progress into the next.
fn exp_for_level(level: i32) -> i32 {
    let l = f64::from(level);
    if level > 30 {
        (4.5 * l * l - 162.5 * l + 2220.0) as i32
    } else if level > 15 {
        (2.5 * l * l - 40.5 * l + 360.0) as i32
    } else {
        level * level + 6 * level
    }
}

fn exp_to_next(level: i32) -> i32 {
    if level >= 30 {
        level * 9 - 158
    } else if level >= 15 {
        level * 5 - 38
    } else {
        level * 2 + 7
    }
}

pub fn total_exp(level: i32, progress: f32) -> i32 {
    exp_for_level(level) + (exp_to_next(level) as f32 * progress).round() as i32
}

fn level_from_exp(exp: i64) -> f64 {
    let e = exp as f64;
    let level = if exp > 1395 {
        ((72.0 * e - 54215.0).sqrt() + 325.0) / 18.0
    } else if exp > 315 {
        (40.0 * e - 7839.0).sqrt() / 10.0 + 8.1
    } else if exp > 0 {
        (e + 9.0).sqrt() - 3.0
    } else {
        0.0
    } as i32;
    let into = exp as f32 - exp_for_level(level) as f32;
    f64::from(level) + f64::from(into / exp_to_next(level) as f32)
}

fn change_exp(player: &Player, by: i32) {
    let now = (i64::from(total_exp(player.get_level(), player.get_exp())) + i64::from(by)).max(0);
    let level = level_from_exp(now);
    player.set_level(level as i32);
    player.set_exp((level - level.trunc()) as f32);
}

/// Every currency currencies.yml registers that this server can pay in.
pub fn load(doc: &Yaml) -> Vec<Currency> {
    let mut out = Vec::new();
    let named = |key: &str| yaml::string(doc, &format!("currencies.{key}.name")).unwrap_or_else(|| format!("&f%price% &#AAFFFF{}", key.to_lowercase()));
    if yaml::boolean(doc, "currencies.Experience.register", false) {
        out.push(Currency { name: "Experience".into(), kind: Kind::Experience, display: named("Experience") });
    }
    if yaml::boolean(doc, "currencies.Vault.register", false) {
        hooks::refresh();
        if hooks::economy::enabled() {
            out.push(Currency { name: "Vault".into(), kind: Kind::Vault, display: named("Vault") });
            crate::texts::console("&#33FF33[AxDarkAuctions] Hooked into Vault!");
        }
    }
    for key in yaml::keys(doc, "placeholder-currencies") {
        let at = |k: &str| format!("placeholder-currencies.{key}.{k}");
        if !yaml::boolean(doc, &at("register"), false) {
            continue;
        }
        out.push(Currency {
            name: key.clone(),
            kind: Kind::Placeholder {
                raw: yaml::string(doc, &at("settings.raw-placeholder")).unwrap_or_default(),
                give: yaml::string(doc, &at("settings.give-command")).unwrap_or_default(),
                take: yaml::string(doc, &at("settings.take-command")).unwrap_or_default(),
                uses_double: yaml::boolean(doc, &at("uses-double"), false),
                offline: yaml::boolean(doc, &at("works-offline"), false),
            },
            display: yaml::string(doc, &at("name")).unwrap_or_else(|| "&f%price%".into()),
        });
        crate::texts::console(&format!("&#33FF33[AxDarkAuctions] Loaded placeholder currency {key}!"));
    }
    if out.is_empty() {
        crate::texts::console("&#FF3333[AxDarkAuctions] Currency hook not found! The plugin will not work properly without an economy plugin!");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn experience_round_trip() {
        assert_eq!(total_exp(0, 0.0), 0);
        assert_eq!(total_exp(15, 0.0), 315);
        assert_eq!(total_exp(30, 0.0), 1395);
        for exp in [0_i64, 7, 100, 315, 900, 1395, 5000] {
            let level = level_from_exp(exp);
            let back = total_exp(level as i32, (level - level.trunc()) as f32);
            assert!((i64::from(back) - exp).abs() <= 1, "{exp} -> {level} -> {back}");
        }
    }
}
