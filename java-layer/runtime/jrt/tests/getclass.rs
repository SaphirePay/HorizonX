use jrt::*;
#[test]
fn get_class_of_things() {
    let one = s("Server Shop");
    let two = s("# a\n# b");
    let arr = new_array("Object", &[V::Int(0)]);
    for (n, v) in [("str", one.clone()), ("multi", two.clone()), ("arr", arr.clone())] {
        let c = call(&v, "getClass", &[]).ok().unwrap();
        println!("{n}: getClass={} key={}", c.text(), c.key());
    }
    let oc = call(&arr, "getClass", &[]).ok().unwrap();
    println!("Object[].isInstance(str)={}", call(&oc, "isInstance", &[one]).ok().unwrap().text());
    println!("Object[].isInstance(multi)={}", call(&oc, "isInstance", &[two]).ok().unwrap().text());
}
