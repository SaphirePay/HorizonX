//! Two things follow the files in `src/commands`, so that adding a command changes nothing else:
//! `$OUT_DIR/command_table.rs`, which lists every file, `$OUT_DIR/bundled_messages.rs`, which lists
//! every language, and `plugin.yml`, which is
//! `plugin-original.yml` with only the commands that have a file. A command with no file is not in
//! `plugin.yml`, so the game doesn't know the word: nothing answers as if it worked.
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::Path;

/// Files in `src/commands` that aren't a command.
const NOT_COMMANDS: [&str; 2] = ["mod", "base"];

fn main() {
    println!("cargo:rerun-if-changed=src/commands");
    println!("cargo:rerun-if-changed=plugin-original.yml");
    println!("cargo:rerun-if-changed=resources/messages");
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("cargo sets the manifest folder");
    let commands = Path::new(&manifest).join("src").join("commands");
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(&commands).expect("src/commands").flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default().to_owned();
        if !NOT_COMMANDS.contains(&stem.as_str()) {
            println!("cargo:rerun-if-changed={}", path.display());
            names.insert(stem);
        }
    }
    let mut table = String::new();
    for name in &names {
        let path = commands.join(format!("{name}.rs")).display().to_string().replace('\\', "/");
        // A file named after a Rust keyword: `r` is fine, `break`, `ref` and such take `cmd_` in front.
        table.push_str(&format!("#[path = \"{path}\"]\npub mod {name};\n"));
    }
    table.push_str("pub const TABLE: &[crate::commands::base::Command] = &[\n");
    for name in &names {
        table.push_str(&format!("    {name}::COMMAND,\n"));
    }
    table.push_str("];\n");
    let out = env::var("OUT_DIR").expect("cargo sets the output folder");
    fs::write(Path::new(&out).join("command_table.rs"), table).expect("the command table");

    let messages = Path::new(&manifest).join("resources").join("messages");
    let languages: BTreeSet<String> = fs::read_dir(&messages).expect("resources/messages").flatten().filter_map(|entry| entry.file_name().to_str()?.strip_suffix(".properties").map(str::to_owned)).collect();
    let mut bundled = String::from("/// Every `messages*.properties` of the original, built in as they are in its jar.\nconst BUNDLED: &[(&str, &str)] = &[\n");
    for name in &languages {
        bundled.push_str(&format!("    ({name:?}, include_str!({:?})),\n", messages.join(format!("{name}.properties")).display().to_string().replace('\\', "/")));
    }
    bundled.push_str("];\n");
    fs::write(Path::new(&out).join("bundled_messages.rs"), bundled).expect("the language list");

    let original = fs::read_to_string(Path::new(&manifest).join("plugin-original.yml")).expect("plugin-original.yml");
    let wanted: BTreeSet<String> = names.iter().map(|name| name.strip_prefix("cmd_").unwrap_or(name).to_owned()).collect();
    let generated = filter(&original, &wanted);
    let target = Path::new(&manifest).join("plugin.yml");
    if fs::read_to_string(&target).ok().as_deref() != Some(generated.as_str()) {
        fs::write(&target, generated).expect("plugin.yml");
    }
}

/// `plugin.yml` keeping only the wanted commands: the original's indentation is two spaces.
fn filter(original: &str, wanted: &BTreeSet<String>) -> String {
    let mut out = String::with_capacity(original.len());
    let mut in_commands = false;
    let mut keeping = true;
    for line in original.lines() {
        if !line.starts_with(' ') && !line.is_empty() {
            in_commands = line.trim_end() == "commands:";
            keeping = true;
        } else if in_commands && line.starts_with("  ") && !line.starts_with("   ") {
            keeping = wanted.contains(line.trim().trim_end_matches(':'));
        }
        if keeping {
            if line.starts_with("version:") {
                out.push_str("version: 2.22.0\n");
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}
