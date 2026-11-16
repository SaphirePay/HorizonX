//! Paper's `getLifecycleManager().registerEventHandler(LifecycleEvents.COMMANDS, event -> ...)`: the
//! handler runs as the plugin enables, which is when the SDK takes command trees, with an event whose
//! `registrar()` registers Brigadier trees for this plugin.
use horizon_bukkit::brigadier::CommandNode;
use horizon_bukkit::fam_paper::Commands;
use jrt::{R, V};

/// The `COMMANDS` event a handler is given.
pub struct CommandsEventH;
/// `event.registrar()`: Paper's `Commands`.
pub struct CommandsH;

fn register(a: &[V]) -> Option<R> {
    if let Some(found) = register_basic(a) {
        return Some(found);
    }
    // The tree comes first, or after the plugin's meta (`register(pluginMeta, node, ...)`).
    let node = a.iter().find_map(|v| crate::held::<CommandNode>(v).map(|n| n.clone()))?;
    // `register(node)`, `register(node, description)`, `register(node, aliases)`, `register(node, description, aliases)`.
    let aliases: Vec<String> = a.iter().skip(1).find(|v| matches!(v, V::List(_) | V::Set(_) | V::Arr(_))).and_then(|v| v.items().ok()).unwrap_or_default().iter().map(V::text).collect();
    let aliases: Vec<&str> = aliases.iter().map(String::as_str).collect();
    let labels = Commands::of(&crate::the_plugin()).register(&node, &aliases);
    Some(Ok(jrt::list(labels.iter().map(|label| jrt::s(label)).collect())))
}

/// Paper's `CommandSourceStack`, as a `BasicCommand` gets it: who sent the command.
pub struct SourceH(V);

/// `registrar.register(label, [description], [aliases], basicCommand)`: Paper's `BasicCommand`, a plugin
/// object with `execute(source, args)`, and maybe `suggest(source, args)`, `permission()`, `canUse(sender)`.
fn register_basic(a: &[V]) -> Option<R> {
    let label = match a.first() {
        Some(V::Str(label)) => label.to_string(),
        _ => return None,
    };
    let object = a.last().filter(|v| matches!(v, V::Obj(_)) && a.len() >= 2)?.clone();
    let description = a.get(1).filter(|_| a.len() >= 3).filter(|v| matches!(v, V::Str(_))).map(V::text);
    let aliases: Vec<String> = a[1..a.len() - 1].iter().find(|v| matches!(v, V::List(_) | V::Set(_) | V::Arr(_))).and_then(|v| v.items().ok()).unwrap_or_default().iter().map(V::text).collect();
    let plugin = crate::the_plugin();
    let mut command = horizon_bukkit::pure2::Command::new(&label);
    command.set_aliases(&aliases.iter().map(String::as_str).collect::<Vec<_>>());
    if let Some(description) = &description {
        command.set_description(description);
    }
    if let Ok(permission) = jrt::call(&object, "permission", &[]) {
        if !permission.is_null() {
            command.set_permission(&permission.text());
        }
    }
    let registered = horizon_bukkit::fam_core::get_command_map().register(&plugin, &plugin.get_name().to_lowercase(), &command);
    let runs = object.clone();
    plugin.set_executor(&label, move |sender, _label, args| {
        let source = jrt::native(SourceH(crate::wrap(sender.clone())));
        // `canUse(sender)` refused: Paper answers as for a missing permission.
        if jrt::call(&runs, "canUse", &[crate::wrap(sender.clone())]).is_ok_and(|ok| !ok.truth()) {
            sender.send_message("\u{a7}cI'm sorry, but you do not have permission to perform this command. Please contact the server administrators if you believe that this is in error.");
            return true;
        }
        let list = jrt::arr(args.iter().map(|x| jrt::s(x)).collect());
        crate::command_outcome(sender, "execute", jrt::call(&runs, "execute", &[source, list]));
        crate::finish_while_paused();
        true
    });
    let completes = object;
    plugin.set_tab_completer(&label, move |sender, _alias, args| {
        let source = jrt::native(SourceH(crate::wrap(sender.clone())));
        let list = jrt::arr(args.iter().map(|x| jrt::s(x)).collect());
        jrt::call(&completes, "suggest", &[source, list]).ok().and_then(|v| v.items().ok()).unwrap_or_default().iter().map(V::text).collect()
    });
    Some(Ok(V::Bool(registered)))
}

/// Brigadier's argument types, as the SDK's: `StringArgumentType.greedyString()`, `ArgumentTypes.players()`...
pub fn static_method(class: &str, name: &str, a: &[V]) -> Option<R> {
    use horizon_bukkit::brigadier::ArgumentType;
    // `requires(Commands.restricted(source -> ...))`: the test itself (the click confirmation it adds
    // isn't kept).
    if class == "Commands" && name == "restricted" && a.len() == 1 {
        return Some(Ok(a[0].clone()));
    }
    let number = |at: usize, or: f64| a.get(at).map_or(or, V::double);
    let kind = match (class, name) {
        ("StringArgumentType", "word" | "string") => ArgumentType::Word,
        ("StringArgumentType", "greedyString") => ArgumentType::Greedy,
        ("IntegerArgumentType" | "LongArgumentType", "integer" | "longArg") => ArgumentType::Integer { min: number(0, f64::from(i32::MIN)) as i32, max: number(1, f64::from(i32::MAX)) as i32 },
        ("DoubleArgumentType" | "FloatArgumentType", "doubleArg" | "floatArg") => ArgumentType::Double { min: number(0, f64::MIN), max: number(1, f64::MAX) },
        ("BoolArgumentType", "bool") => ArgumentType::Bool,
        ("ArgumentTypes", "player") => ArgumentType::Player,
        ("ArgumentTypes", "players") => ArgumentType::Players,
        // A name, which may be a player who isn't on: one word, as the client sends it.
        ("ArgumentTypes", "playerProfiles") => ArgumentType::Word,
        _ => return None,
    };
    Some(Ok(crate::wrap(kind)))
}

/// Brigadier's `SimpleCommandExceptionType(message)` and `DynamicCommandExceptionType(fn)`: `create(...)`
/// is the `CommandSyntaxException` a command throws, with its message.
pub struct CommandErrorH(V);

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    matches!(class, "SimpleCommandExceptionType" | "DynamicCommandExceptionType" | "Dynamic2CommandExceptionType")
        .then(|| Ok(jrt::native(CommandErrorH(a.first().cloned().unwrap_or(V::Null)))))
}

fn command_error(kind: &CommandErrorH, name: &str, a: &[V]) -> Option<R> {
    if !matches!(name, "create" | "createWithContext") {
        return None;
    }
    let args: Vec<V> = a.iter().filter(|v| !matches!(v, V::Native(_))).cloned().collect();
    let message = match &kind.0 {
        V::Func(_) | V::Obj(_) => jrt::call(&kind.0, "apply", &args).map(|m| crate::legacy_text(&m)).unwrap_or_default(),
        other => crate::legacy_text(other),
    };
    Some(Ok(jrt::exc("CommandSyntaxException", message)))
}

/// A Java lambda (or an object with the interface's method) called with one argument.
fn call_one(f: &V, method: &str, arg: V) -> R {
    let how = if matches!(f, V::Func(_)) { "$invoke" } else { method };
    jrt::call(f, how, &[arg])
}

/// Brigadier's builders, with Java lambdas: `executes(ctx -> ...)`, `requires(source -> ...)`. The SDK's
/// node takes Rust closures; the source a Paper lambda is given is the sender here (its `getSender()` is
/// itself).
fn node_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    let mut node = crate::held::<CommandNode>(this)?;
    match (name, a.len()) {
        ("executes", 1) if matches!(a[0], V::Func(_) | V::Obj(_)) => {
            let run = a[0].clone();
            *node = node.clone().executes(move |context| match call_one(&run, "run", crate::wrap(context.clone())) {
                Ok(value) => value.int(),
                Err(error) => {
                    let _ = jrt::report("command", Err(error));
                    0
                }
            });
        }
        ("requires", 1) if matches!(a[0], V::Func(_) | V::Obj(_)) => {
            let test = a[0].clone();
            *node = node.clone().requires(move |sender| call_one(&test, "test", crate::wrap(sender.clone())).is_ok_and(|v| v.truth()));
        }
        ("then", 1) => {
            let child = crate::held::<CommandNode>(&a[0])?.clone();
            *node = node.clone().then(child);
        }
        // Suggestions are the client's: the tree it is sent already completes names and literals. A builder
        // and the node it builds are one value here.
        ("suggests", 1) | ("build", 0) => {}
        _ => return None,
    }
    drop(node);
    Some(Ok(this.clone()))
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(kind) = this.native::<CommandErrorH>() {
        return command_error(kind, name, a);
    }
    if let Some(found) = node_method(this, name, a) {
        return Some(found);
    }
    // A command's source is its sender here: `ctx.getSource().getSender()`.
    if matches!(name, "getSender" | "getExecutor") && a.is_empty() {
        if let Some(sender) = crate::held::<horizon_bukkit::CommandSender>(this) {
            return Some(Ok(match (name, &*sender) {
                ("getSender", _) => this.clone(),
                (_, horizon_bukkit::CommandSender::Player(player)) => crate::wrap(player.clone()),
                _ => V::Null,
            }));
        }
    }
    if name == "registerEventHandler" && a.len() == 2 && this.native::<std::sync::Mutex<horizon_bukkit::fam_paper::LifecycleEventManager>>().is_some() {
        let event = a[0].text();
        if !event.contains("COMMANDS") {
            // Registry and bootstrap moments: nothing here gives them, as on a server without them.
            return Some(Ok(V::Null));
        }
        let how = if matches!(a[1], V::Func(_)) { "$invoke" } else { "run" };
        return Some(jrt::call(&a[1], how, &[jrt::native(CommandsEventH)]).map(|_| V::Null));
    }
    if this.native::<CommandsEventH>().is_some() {
        return Some(Ok(match name {
            "registrar" => jrt::native(CommandsH),
            "cause" => jrt::s("INITIAL"),
            "owner" => crate::main_object(),
            _ => return None,
        }));
    }
    if let Some(source) = this.native::<SourceH>() {
        let player = jrt::instance_of(&source.0, "Player");
        return Some(match name {
            "getSender" => Ok(source.0.clone()),
            "getExecutor" => Ok(if player { source.0.clone() } else { V::Null }),
            "getLocation" => jrt::call(&source.0, "getLocation", &[]),
            _ => return None,
        });
    }
    if this.native::<CommandsH>().is_some() {
        return match name {
            "register" | "registerWithFlags" => register(a),
            "getDispatcher" => Some(Ok(crate::wrap(Commands::of(&crate::the_plugin()).get_dispatcher()))),
            _ => None,
        };
    }
    None
}
