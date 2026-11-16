//! How gradients and legacy hex reach the client: straight through parse_mixed, and after text::colorize.
use horizon_kit::{mini, text};

#[test]
fn gradients_keep_their_colours() {
    let direct = mini::parse_mixed("<gradient:#a855f7:#22d3ee>NYXORA</gradient>").to_string();
    let colorized = mini::parse_mixed(&text::colorize("<gradient:#a855f7:#22d3ee>NYXORA</gradient>")).to_string();
    let legacy_hex = mini::parse_mixed("&#FF0000Red &aGreen").to_string();
    println!("direct:     {direct}");
    println!("colorized:  {colorized}");
    println!("legacy hex: {legacy_hex}");
    assert!(direct.contains("#a855f7") || direct.contains("#A855F7"), "{direct}");
    assert!(legacy_hex.contains("#ff0000") || legacy_hex.contains("#FF0000") || legacy_hex.contains("red"), "{legacy_hex}");
}
