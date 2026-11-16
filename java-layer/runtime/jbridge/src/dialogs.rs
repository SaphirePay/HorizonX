//! Paper's `Dialog.create(builder -> builder.empty().base(...).type(...))`: the builder the lambda fills,
//! made into the SDK's `Dialog` once it returns.
use horizon_bukkit::dialog::{Dialog, DialogBase, DialogType};
use jrt::{R, V};
use std::sync::Mutex;

/// `DialogRegistryEntry.Builder`, as the factory's `empty()` gives it.
pub struct DialogBuilderH(Mutex<(Option<DialogBase>, Option<DialogType>)>);

pub fn static_method(class: &str, name: &str, a: &[V]) -> Option<R> {
    if class != "Dialog" || name != "create" || a.len() != 1 || !matches!(a[0], V::Func(_) | V::Obj(_)) {
        return None;
    }
    let builder = jrt::native(DialogBuilderH(Mutex::new((None, None))));
    let how = if matches!(a[0], V::Func(_)) { "$invoke" } else { "accept" };
    if let Err(error) = jrt::call(&a[0], how, std::slice::from_ref(&builder)) {
        return Some(Err(error));
    }
    let made = builder.native::<DialogBuilderH>()?;
    let (base, kind) = jrt::lock(&made.0).clone();
    match (base, kind) {
        (Some(base), Some(kind)) => Some(Ok(crate::wrap(Dialog::create(base, kind)))),
        _ => Some(Err(jrt::exc("IllegalStateException", "a dialog needs a base and a type"))),
    }
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    let builder = this.native::<DialogBuilderH>()?;
    match (name, a.len()) {
        ("empty", 0) => Some(Ok(this.clone())),
        ("base", 1) => {
            let base = crate::held::<DialogBase>(&a[0])?.clone();
            jrt::lock(&builder.0).0 = Some(base);
            Some(Ok(this.clone()))
        }
        ("type", 1) => {
            let kind = crate::held::<DialogType>(&a[0])?.clone();
            jrt::lock(&builder.0).1 = Some(kind);
            Some(Ok(this.clone()))
        }
        _ => None,
    }
}
