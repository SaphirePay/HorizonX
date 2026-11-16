//! `super.clone()` up to Object, and ForkJoinTask's statics (FAWE's ApplyTask).
use jrt::*;

fn m_clone_base(this: &V, _a: &[V]) -> R {
    call_super(c_base(), this, "clone", &[])
}
fn m_clone_sub(this: &V, _a: &[V]) -> R {
    call_super(c_sub(), this, "clone", &[])
}
fn find_base(name: &str, _a: &[V]) -> Option<Method> {
    (name == "clone").then_some(m_clone_base as Method)
}
fn find_sub(name: &str, _a: &[V]) -> Option<Method> {
    (name == "clone").then_some(m_clone_sub as Method)
}
fn c_base() -> &'static Class {
    static C: Class = Class { name: "AbstractRegion", parent: None, interfaces: &[], types: &["AbstractRegion", "Cloneable"], find: find_base };
    &C
}
fn c_sub() -> &'static Class {
    static C: Class = Class { name: "CuboidRegion", parent: Some(c_base), interfaces: &[], types: &["CuboidRegion", "AbstractRegion", "Cloneable"], find: find_sub };
    &C
}

#[test]
fn clone_and_forkjoin() {
    let r = new_obj(c_sub(), &[], &[]).ok().unwrap();
    r.set("x", V::Int(7)).ok();
    let copy = call(&r, "clone", &[]);
    println!("clone -> {:?}", copy.as_ref().map(|v| v.text()).map_err(|e| e.text()));
    let copy = copy.ok().unwrap();
    assert!(matches!(copy, V::Obj(_)), "clone gave {}", copy.text());
    assert_eq!(copy.get("x").ok().unwrap().text(), "7");
    let surplus = call_static("ForkJoinTask", "getSurplusQueuedTaskCount", &[]);
    println!("surplus -> {:?}", surplus.as_ref().map(|v| v.text()).map_err(|e| e.text()));
    assert_eq!(surplus.ok().unwrap().text(), i32::MAX.to_string());
}
