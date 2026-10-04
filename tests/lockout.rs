//! How long a device waits after wrong passphrases.

use erisauth::plugin::state::{FREE_ATTEMPTS, Lockout, MAX_LOCK_SECS};

#[test]
fn waits_start_at_a_minute_double_and_stop_at_a_day() {
    let now = 1_000_000;
    let mut lockout = Lockout::default();
    for _ in 1..FREE_ATTEMPTS {
        lockout.fail(now);
        assert_eq!(lockout.wait(now), None, "the first guesses are free");
    }
    let mut waits = Vec::new();
    for _ in 0..14 {
        lockout.fail(now);
        waits.push(lockout.wait(now).unwrap());
    }
    assert_eq!(&waits[..5], &[60, 120, 240, 480, 960]);
    assert_eq!(*waits.last().unwrap(), MAX_LOCK_SECS);
    assert!(waits.windows(2).all(|w| w[0] <= w[1]));

    lockout.fail(now);
    assert_eq!(lockout.wait(now + MAX_LOCK_SECS), None, "a wait ends");
    for _ in 0..100 {
        lockout.fail(now);
    }
    assert_eq!(lockout.wait(now), Some(MAX_LOCK_SECS), "no overflow however many guesses");
}
