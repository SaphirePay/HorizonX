//! Kotlin's standard library as a Kotlin plugin calls it once compiled to Java: the null checks the compiler
//! adds (`Intrinsics`), `Pair` and `to`, and the collection builders `listOf`, `setOf`, `mapOf`.
use super::*;

/// A call's arguments, Kotlin's `vararg` arriving as one array or spread out.
fn spread(a: &[V]) -> Vec<V> {
    match a {
        [one @ V::Arr(_)] => one.items().unwrap_or_default(),
        many => many.to_vec(),
    }
}

fn built(class: &str, items: Vec<V>, add: &str) -> R {
    let made = new_other(class, &[])?;
    for item in items {
        match (add, &item) {
            ("put", V::Entry(pair)) => {
                call(&made, "put", &[pair.0.clone(), pair.1.clone()])?;
            }
            ("put", _) => {}
            _ => {
                call(&made, add, &[item])?;
            }
        }
    }
    Ok(made)
}

static MISSING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The jar has no Kotlin standard library and its plugin.yml doesn't ask Paper for one: there the first
/// touch of a `kotlin.` class throws NoClassDefFoundError (and an onEnable that does fails the plugin).
pub fn set_missing(missing: bool) {
    MISSING.store(missing, std::sync::atomic::Ordering::Relaxed);
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    let found = stdlib_call(class, name, a)?;
    if MISSING.load(std::sync::atomic::Ordering::Relaxed) {
        let package = if class == "Intrinsics" { "kotlin/jvm/internal" } else { "kotlin" };
        return Some(Err(exc("NoClassDefFoundError", format!("{package}/{class}"))));
    }
    Some(found)
}

fn stdlib_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    Some(match (class, name) {
        ("Intrinsics", "areEqual") => {
            let (left, right) = (a.first().cloned().unwrap_or(V::Null), a.get(1).cloned().unwrap_or(V::Null));
            if left.is_null() { Ok(V::Bool(right.is_null())) } else { call(&left, "equals", &[right]).map(|v| V::Bool(v.truth())) }
        }
        ("Intrinsics", "stringPlus") => Ok(s(&format!("{}{}", a.first().map_or("null".to_owned(), |v| if v.is_null() { "null".into() } else { v.text() }), a.get(1).map_or("null".to_owned(), |v| if v.is_null() { "null".into() } else { v.text() })))),
        ("Intrinsics", "compare") => Ok(V::Int(a[0].long().cmp(&a[1].long()) as i32)),
        ("Intrinsics", "throwUninitializedPropertyAccessException") => Err(exc("UninitializedPropertyAccessException", format!("lateinit property {} has not been initialized", a.first().map_or(String::new(), V::text)))),
        ("Intrinsics", "throwNpe" | "throwJavaNpe") => Err(exc("NullPointerException", "")),
        ("Intrinsics", check) if check.starts_with("check") => {
            // The checks Kotlin adds on every parameter and platform value: a null where none may be is Java's NPE.
            if (check.contains("NotNull") || check.contains("IsNotNull")) && a.first().is_some_and(V::is_null) {
                Err(exc("NullPointerException", format!("{} must not be null", a.get(1).map_or("value".to_owned(), V::text))))
            } else {
                Ok(V::Null)
            }
        }
        ("TuplesKt", "to") | ("Pair" | "TuplesKt", "of") => Ok(V::Entry(Arc::new((a[0].clone(), a[1].clone())))),
        ("CollectionsKt", "listOf" | "mutableListOf" | "arrayListOf") => Ok(list(spread(a))),
        ("CollectionsKt", "listOfNotNull") => Ok(list(spread(a).into_iter().filter(|v| !v.is_null()).collect())),
        ("CollectionsKt", "emptyList") => Ok(list(Vec::new())),
        ("SetsKt", "setOf" | "mutableSetOf" | "hashSetOf" | "linkedSetOf") => built("LinkedHashSet", spread(a), "add"),
        ("SetsKt", "emptySet") => built("LinkedHashSet", Vec::new(), "add"),
        ("MapsKt", "mapOf" | "mutableMapOf" | "hashMapOf" | "linkedMapOf") => built("LinkedHashMap", spread(a), "put"),
        ("MapsKt", "emptyMap") => built("LinkedHashMap", Vec::new(), "put"),
        _ => return None,
    })
}

/// Kotlin's `Pair`, kept as a map entry: `first`, `second`, and the `component` functions of destructuring.
pub fn method(this: &V, name: &str, _a: &[V]) -> Option<R> {
    let V::Entry(pair) = this else { return None };
    Some(Ok(match name {
        "getFirst" | "component1" => pair.0.clone(),
        "getSecond" | "component2" => pair.1.clone(),
        _ => return None,
    }))
}
