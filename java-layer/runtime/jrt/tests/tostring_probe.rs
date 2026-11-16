use jrt::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::collections::BTreeMap;

static COUNT: AtomicU32 = AtomicU32::new(0);

fn to_string(_this: &V, _a: &[V]) -> R {
    COUNT.fetch_add(1, Ordering::SeqCst);
    Ok(s("probe"))
}

fn find(name: &str, _a: &[V]) -> Option<Method> {
    (name == "toString").then_some(to_string as Method)
}

static PROBE: Class = Class { name: "TextComponentImpl", parent: None, interfaces: &[], types: &["TextComponentImpl", "Component"], find };

fn obj() -> V {
    V::Obj(Arc::new(Object { class: &PROBE, fields: Mutex::new(BTreeMap::new()) }))
}

#[test]
fn builder_add_does_not_stringify() {
    let builder = call_static("ImmutableList", "builder", &[]).ok().unwrap();
    for (what, args) in [("add", vec![obj()]), ("addAll", vec![list(vec![obj(), obj()])]), ("add", vec![obj(), obj()])] {
        COUNT.store(0, Ordering::SeqCst);
        call(&builder, what, &args).ok().unwrap();
        println!("{what}({}) -> toString x{}", args.len(), COUNT.load(Ordering::SeqCst));
    }
    COUNT.store(0, Ordering::SeqCst);
    let built = call(&builder, "build", &[]).ok().unwrap();
    println!("build -> toString x{}", COUNT.load(Ordering::SeqCst));
    COUNT.store(0, Ordering::SeqCst);
    let _ = call_static("ImmutableList", "copyOf", &[built]);
    println!("copyOf -> toString x{}", COUNT.load(Ordering::SeqCst));
}
