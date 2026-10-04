use super::slider_gain;

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
