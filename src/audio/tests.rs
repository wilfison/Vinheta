use std::time::Duration;

use super::{retry_delay, slider_gain};

#[test]
fn slider_gain_is_cubic() {
    assert_eq!(slider_gain(0.0), 0.0);
    assert_eq!(slider_gain(1.0), 1.0);
    assert_eq!(slider_gain(0.5), 0.125);
}

#[test]
fn slider_gain_clamps_the_position() {
    assert_eq!(slider_gain(-0.5), 0.0);
    assert_eq!(slider_gain(1.5), 1.0);
}

#[test]
fn retry_delay_doubles_up_to_half_a_minute() {
    let seconds = |attempt| retry_delay(attempt).as_secs();
    assert_eq!(seconds(0), 1);
    assert_eq!(seconds(1), 2);
    assert_eq!(seconds(4), 16);
    assert_eq!(seconds(5), 30);
    assert_eq!(retry_delay(63), Duration::from_secs(30));
    assert_eq!(retry_delay(64), Duration::from_secs(30));
    assert_eq!(retry_delay(u32::MAX), Duration::from_secs(30));
}
