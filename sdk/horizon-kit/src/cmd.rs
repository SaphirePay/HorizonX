//! A command with sub-commands, as most plugins have (`/dh create <name>`, `/home set`): each sub its names,
//! permission, usage, how many arguments it needs and whether only a player may run it; the help page, the
//! permission and usage errors and tab completion come from that table.
//!
//! ```ignore
//! static ROUTER: LazyLock<Router> = LazyLock::new(|| Router::new("warp")
//!     .sub(Sub::new("set", set).perm("warp.set").usage("set <name>").args(1).player().about("Make a warp here"))
//!     .sub(Sub::new("list", list).alias("ls").about("All warps")));
//! plugin.set_executor("warp", |s, l, a| ROUTER.run(s, l, a));
//! plugin.set_tab_completer("warp", |s, _, a| ROUTER.complete(s, a));
//! ```

use horizon_bukkit::{CommandSender, Player};

/// What a sub's handler is given.
pub struct Ctx<'a> {
    pub sender: &'a CommandSender,
    pub label: &'a str,
    /// The sub-command's name as typed (empty for the bare command).
    pub sub: String,
    /// The arguments after the sub's name.
    pub args: Vec<String>,
}

pub type Res = Result<(), String>;

impl Ctx<'_> {
    pub fn tell(&self, text: &str) {
        for line in text.split('\n') {
            self.sender.send_message(&crate::text::colorize(line));
        }
    }
    pub fn player(&self) -> Result<Player, String> {
        match self.sender {
            CommandSender::Player(p) => Ok(p.clone()),
            _ => Err("&cOnly players can do that.".into()),
        }
    }
    pub fn arg(&self, i: usize) -> Option<&str> {
        self.args.get(i).map(String::as_str)
    }
    /// The arguments from `i` on, joined by spaces (a message, a name with spaces).
    pub fn rest(&self, i: usize) -> String {
        self.args.get(i..).map(|a| a.join(" ")).unwrap_or_default()
    }
    pub fn int(&self, i: usize, what: &str) -> Result<i64, String> {
        self.arg(i).and_then(|s| s.trim().parse().ok()).ok_or_else(|| format!("&c{what} must be a whole number."))
    }
    pub fn num(&self, i: usize, what: &str) -> Result<f64, String> {
        self.arg(i).and_then(|s| s.trim().replace(',', ".").parse().ok()).ok_or_else(|| format!("&c{what} must be a number."))
    }
    pub fn bool(&self, i: usize, what: &str) -> Result<bool, String> {
        match self.arg(i).map(str::to_ascii_lowercase).as_deref() {
            Some("true" | "yes" | "on" | "1") => Ok(true),
            Some("false" | "no" | "off" | "0") => Ok(false),
            _ => Err(format!("&c{what} must be true or false.")),
        }
    }
    /// An online player by name (any case).
    pub fn online(&self, i: usize) -> Result<Player, String> {
        let name = self.arg(i).unwrap_or_default();
        horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(name)).ok_or_else(|| format!("&cNo player named {name} is online."))
    }
}

pub type Handler = fn(&Ctx) -> Res;
pub type Completer = fn(&Ctx) -> Vec<String>;

pub struct Sub {
    pub names: Vec<&'static str>,
    pub perm: Option<&'static str>,
    pub usage: &'static str,
    pub about: &'static str,
    pub min_args: usize,
    pub player_only: bool,
    pub run: Handler,
    pub complete: Option<Completer>,
    pub hidden: bool,
}

impl Sub {
    pub fn new(name: &'static str, run: Handler) -> Sub {
        Sub { names: vec![name], perm: None, usage: name, about: "", min_args: 0, player_only: false, run, complete: None, hidden: false }
    }
    pub fn alias(mut self, name: &'static str) -> Sub {
        self.names.push(name);
        self
    }
    pub fn perm(mut self, node: &'static str) -> Sub {
        self.perm = Some(node);
        self
    }
    pub fn usage(mut self, usage: &'static str) -> Sub {
        self.usage = usage;
        self
    }
    pub fn about(mut self, about: &'static str) -> Sub {
        self.about = about;
        self
    }
    pub fn args(mut self, n: usize) -> Sub {
        self.min_args = n;
        self
    }
    pub fn player(mut self) -> Sub {
        self.player_only = true;
        self
    }
    pub fn hidden(mut self) -> Sub {
        self.hidden = true;
        self
    }
    pub fn tab(mut self, complete: Completer) -> Sub {
        self.complete = Some(complete);
        self
    }
}

/// The messages a router says; plugins give their own (from their lang file) with [`Router::messages`].
pub struct Messages {
    pub no_permission: String,
    pub usage: String,
    pub unknown: String,
    pub player_only: String,
    pub help_title: String,
    pub help_line: String,
}

impl Default for Messages {
    fn default() -> Self {
        Messages {
            no_permission: "&cYou don't have permission to do that.".into(),
            usage: "&cUsage: &7/{label} {usage}".into(),
            unknown: "&cUnknown sub-command. &7/{label} help".into(),
            player_only: "&cOnly players can do that.".into(),
            help_title: "&3&l{command} &7commands:".into(),
            help_line: " &8• &b/{label} {usage} &8- &7{about}".into(),
        }
    }
}

pub struct Router {
    pub command: &'static str,
    pub subs: Vec<Sub>,
    /// What runs with no arguments (`help` when none).
    pub bare: Option<Handler>,
    /// A permission over all of it (`plugin.admin`): one who has it may run any sub.
    pub admin: Option<&'static str>,
    pub messages: Messages,
}

impl Router {
    pub fn new(command: &'static str) -> Router {
        Router { command, subs: Vec::new(), bare: None, admin: None, messages: Messages::default() }
    }
    pub fn sub(mut self, sub: Sub) -> Router {
        self.subs.push(sub);
        self
    }
    pub fn bare(mut self, run: Handler) -> Router {
        self.bare = Some(run);
        self
    }
    pub fn admin(mut self, node: &'static str) -> Router {
        self.admin = Some(node);
        self
    }
    pub fn messages(mut self, m: Messages) -> Router {
        self.messages = m;
        self
    }

    fn find(&self, name: &str) -> Option<&Sub> {
        self.subs.iter().find(|s| s.names.iter().any(|n| n.eq_ignore_ascii_case(name)))
    }

    fn may(&self, sender: &CommandSender, sub: &Sub) -> bool {
        sub.perm.is_none_or(|p| sender.has_permission(p)) || self.admin.is_some_and(|a| sender.has_permission(a))
    }

    fn fill(&self, text: &str, label: &str, sub: Option<&Sub>) -> String {
        crate::fmt::named(text, &[("label", label), ("command", self.command), ("usage", sub.map_or("", |s| s.usage)), ("about", sub.map_or("", |s| s.about))])
    }

    /// The help page: every sub this sender may run.
    pub fn help(&self, sender: &CommandSender, label: &str) {
        sender.send_message(&crate::text::colorize(&self.fill(&self.messages.help_title, label, None)));
        for sub in self.subs.iter().filter(|s| !s.hidden && self.may(sender, s)) {
            sender.send_message(&crate::text::colorize(&self.fill(&self.messages.help_line, label, Some(sub))));
        }
    }

    /// For `set_executor`: always `true` (the router says what went wrong itself).
    pub fn run(&self, sender: &CommandSender, label: &str, args: &[&str]) -> bool {
        let started = std::time::Instant::now();
        let say = |t: &str| {
            for line in t.split('\n') {
                sender.send_message(&crate::text::colorize(line));
            }
        };
        let Some(first) = args.first() else {
            match self.bare {
                Some(run) => {
                    if let Err(e) = run(&Ctx { sender, label, sub: String::new(), args: Vec::new() }) {
                        say(&e);
                    }
                }
                None => self.help(sender, label),
            }
            return true;
        };
        let Some(sub) = self.find(first) else {
            if first.eq_ignore_ascii_case("help") || *first == "?" {
                self.help(sender, label);
            } else {
                say(&self.fill(&self.messages.unknown, label, None));
            }
            return true;
        };
        if !self.may(sender, sub) {
            say(&self.fill(&self.messages.no_permission, label, Some(sub)));
            return true;
        }
        if sub.player_only && !matches!(sender, CommandSender::Player(_)) {
            say(&self.fill(&self.messages.player_only, label, Some(sub)));
            return true;
        }
        let rest: Vec<String> = args[1..].iter().map(|a| (*a).to_owned()).collect();
        if rest.len() < sub.min_args {
            say(&self.fill(&self.messages.usage, label, Some(sub)));
            return true;
        }
        if let Err(e) = (sub.run)(&Ctx { sender, label, sub: (*first).to_owned(), args: rest }) {
            say(&e);
        }
        crate::stats::time("command", started.elapsed());
        true
    }

    /// For `set_tab_completer`: the subs (that this sender may run) for the first word, then the sub's own.
    pub fn complete(&self, sender: &CommandSender, args: &[&str]) -> Vec<String> {
        match args {
            [] => Vec::new(),
            [first] => {
                let lower = first.to_ascii_lowercase();
                self.subs.iter().filter(|s| !s.hidden && self.may(sender, s)).flat_map(|s| s.names.first().copied()).filter(|n| n.starts_with(&lower)).map(str::to_owned).collect()
            }
            [first, rest @ ..] => match self.find(first).filter(|s| self.may(sender, s)).and_then(|s| s.complete) {
                Some(complete) => {
                    let last = rest.last().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
                    complete(&Ctx { sender, label: self.command, sub: (*first).to_owned(), args: rest.iter().map(|a| (*a).to_owned()).collect() }).into_iter().filter(|c| c.to_ascii_lowercase().starts_with(&last)).collect()
                }
                None => Vec::new(),
            },
        }
    }
}

/// Online players' names (a common completer).
pub fn players(_: &Ctx) -> Vec<String> {
    horizon_bukkit::get_online_players().into_iter().map(|p| p.get_name().to_owned()).collect()
}
