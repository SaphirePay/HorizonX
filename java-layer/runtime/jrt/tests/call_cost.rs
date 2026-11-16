//! What one converted call costs: `cargo test --release --test call_cost -- --nocapture`.
//! A class with a field `x` and `getX()`, called the way jport writes it (by name) and directly.
use jrt::*;
use std::time::Instant;

fn m_get_x(this: &V, _a: &[V]) -> R {
    this.get("x")
}
fn m_add(this: &V, a: &[V]) -> R {
    Ok(add(&this.get("x")?, &a[0]))
}
fn find(name: &str, _a: &[V]) -> Option<Method> {
    match name {
        "getX" => Some(m_get_x),
        "add" => Some(m_add),
        _ => None,
    }
}
fn c_point() -> &'static Class {
    static C: Class = Class { name: "Point", parent: None, interfaces: &[], types: &["Point"], find };
    &C
}

#[test]
fn call_cost() {
    println!("-- on a 2 MB thread (the test's own, as the Bukkit async scheduler's)");
    measure();
    println!("-- on a 64 MB thread");
    std::thread::Builder::new().stack_size(64 << 20).spawn(measure).unwrap().join().unwrap();
}

fn measure() {
    let p = new_obj(c_point(), &[], &[]).ok().unwrap();
    p.set("x", V::Int(3)).ok();
    let n = 1_000_000;

    let t = Instant::now();
    let mut sum = V::Int(0);
    for _ in 0..n {
        sum = add(&sum, &call(&p, "getX", &[]).ok().unwrap());
    }
    let by_name = t.elapsed();

    let t = Instant::now();
    let mut sum2 = V::Int(0);
    for _ in 0..n {
        sum2 = add(&sum2, &m_get_x(&p, &[]).ok().unwrap());
    }
    let direct = t.elapsed();

    let t = Instant::now();
    let mut sum3 = V::Int(0);
    for _ in 0..n {
        sum3 = add(&sum3, &p.get("x").ok().unwrap());
    }
    let field = t.elapsed();

    let t = Instant::now();
    let mut acc = 0i64;
    for i in 0..n {
        acc += std::hint::black_box(3i64) + (i & 1);
    }
    let native = t.elapsed();

    let t = Instant::now();
    let mut sum4 = V::Int(0);
    for _ in 0..n {
        sum4 = call(&p, "add", &[sum4]).ok().unwrap();
    }
    let with_arg = t.elapsed();

    let ns = |d: std::time::Duration| d.as_nanos() as f64 / n as f64;
    println!("by name call(p, \"getX\")      {:6.1} ns", ns(by_name));
    println!("same call, args (Int)        {:6.1} ns", ns(with_arg));
    println!("direct m_get_x(p)            {:6.1} ns", ns(direct));
    println!("field p.get(\"x\") + add       {:6.1} ns", ns(field));
    println!("native i64 add               {:6.1} ns   [{} {} {} {} {}]", ns(native), sum.text(), sum2.text(), sum3.text(), sum4.text(), acc);
}
