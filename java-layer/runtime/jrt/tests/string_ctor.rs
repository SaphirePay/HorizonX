use jrt::*;
#[test]
fn string_from_code_points_with_offset() {
    let cps = new_array("int", &[V::Int(5)]);
    for (i, c) in "hello".chars().enumerate() { store(&cps, &V::Int(i as i32), V::Int(c as i32)).ok().unwrap(); }
    let got = new_other("String", &[cps.clone(), V::Int(1), V::Int(3)]).ok().unwrap();
    assert_eq!(got.text(), "ell");
    let chars = new_array("char", &[V::Int(5)]);
    for (i, c) in "hello".chars().enumerate() { store(&chars, &V::Int(i as i32), V::Char(c)).ok().unwrap(); }
    let got2 = new_other("String", &[chars, V::Int(2), V::Int(2)]).ok().unwrap();
    assert_eq!(got2.text(), "ll");
    let window = call_static("Arrays", "copyOfRange", &[cps, V::Int(3), V::Int(8)]).ok().unwrap();
    assert_eq!(window.items().ok().unwrap().iter().map(|v| v.text()).collect::<Vec<_>>(), ["108", "111", "0", "0", "0"]);
}
