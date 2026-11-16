// The prelude in one glob: every common name resolves without a clash.
#[allow(unused_imports)]
use horizon_kit::prelude::*;

#[test]
fn names_resolve() {
    let _v: Value = json!({ "a": 1 });
    let _m: fn(usize, &str) -> gui::Menu = gui::Menu::new;
    let _p: Option<Player> = None;
    let _c: fn(&str) -> Config = Config::parse;
    let _t = time::parse("1m");
    let _s = translate_alternate_color_codes('&', "&a");
    let _y = yaml::int(&serde_yaml::Value::Null, "x", 1);
}
