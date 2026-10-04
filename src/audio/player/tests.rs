use super::effective_gain;

#[test]
fn effective_gain_is_the_product() {
    assert_eq!(effective_gain(1.0, 1.0, 1.0), 1.0);
    assert_eq!(effective_gain(0.5, 0.5, 1.0), 0.25);
    assert_eq!(effective_gain(0.5, 0.5, 0.5), 0.125);
}

#[test]
fn effective_gain_clamps_each_factor() {
    assert_eq!(effective_gain(2.0, 0.5, 1.0), 0.5);
    assert_eq!(effective_gain(0.5, 3.0, 1.0), 0.5);
    assert_eq!(effective_gain(0.5, 1.0, 1.5), 0.5);
    assert_eq!(effective_gain(-1.0, 1.0, 1.0), 0.0);
}

#[test]
fn effective_gain_is_zero_when_a_factor_is_zero() {
    assert_eq!(effective_gain(0.0, 1.0, 1.0), 0.0);
    assert_eq!(effective_gain(1.0, 0.0, 1.0), 0.0);
    assert_eq!(effective_gain(1.0, 1.0, 0.0), 0.0);
}
