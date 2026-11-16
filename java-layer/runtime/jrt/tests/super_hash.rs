use jrt::*;
fn m_hash(this: &V, _a: &[V]) -> R { call_super(c_node(), this, "hashCode", &[]) }
fn m_text(_this: &V, _a: &[V]) -> R { Ok(s("same")) }
fn find(name: &str, _a: &[V]) -> Option<Method> { match name { "hashCode" => Some(m_hash), "toString" => Some(m_text), _ => None } }
fn c_node() -> &'static Class { static C: Class = Class { name: "Node", parent: None, interfaces: &[], types: &["Node"], find }; &C }
#[test]
fn super_hash_code_is_identity() {
    let a = new_obj(c_node(), &[], &[]).ok().unwrap();
    let b = new_obj(c_node(), &[], &[]).ok().unwrap();
    let ha = call(&a, "hashCode", &[]);
    let hb = call(&b, "hashCode", &[]);
    println!("a={:?} b={:?} keys {} {}", ha.map(|v| v.text()).ok(), hb.map(|v| v.text()).ok(), a.key(), b.key());
    assert_ne!(a.key(), b.key());
}
