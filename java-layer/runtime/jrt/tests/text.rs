//! DecimalFormat against what Java 21 prints for the same pattern and number.
use jrt::{s, V};

fn fmt(pattern: &str, value: f64) -> String {
    let made = match jrt::text::construct("DecimalFormat", &[s(pattern)]).unwrap() {
        Ok(v) => v,
        Err(e) => panic!("{}", e.text()),
    };
    match jrt::text::method(&made, "format", &[V::Double(value)]).unwrap() {
        Ok(v) => v.text(),
        Err(e) => panic!("{}", e.text()),
    }
}

#[test]
fn like_java() {
    let cases = [
        ("#.##", 2.675, "2.67"),
        ("#.##", 3.0, "3"),
        ("#.##", 0.5, ".5"),
        ("#.##", 0.125, ".12"),
        ("0.00", 3.14159, "3.14"),
        ("0.00", 2.0, "2.00"),
        ("#,###", 1234567.0, "1,234,567"),
        ("#,##0.00", 1234.5, "1,234.50"),
        ("#,##0.00", -0.004, "-0.00"),
        ("#.#", 19.96, "20"),
        ("0.0", 19.96, "20.0"),
        ("#", 0.4, "0"),
        ("#%", 0.256, "26%"),
        ("$#,##0.00", 1500.0, "$1,500.00"),
        ("#.##", -1.5, "-1.5"),
    ];
    for (pattern, value, java) in cases {
        assert_eq!(fmt(pattern, value), java, "{pattern} of {value}");
    }
}
