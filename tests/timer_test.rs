/// Test the timer interface

extern crate amy;

use std::time::{Instant, Duration};

use amy::Poller;

const TIMEOUT: usize = 50; // ms
const POLL_TIMEOUT: usize = 5000; // ms

// Use a shorter timeout, because we don't want to wait a whole 5 seconds
// It's still longer than the poll timeout though. We have two timeouts so that we don't fail
// earlier tests due to ridiculously slow machines.
const FINAL_POLL_TIMEOUT: usize = 250; // ms

#[test]
fn test_set_timeout() {
    let mut poller = Poller::new().unwrap();
    let registrar = poller.get_registrar();
    let now = Instant::now();
    let timer = registrar.set_timeout(TIMEOUT).unwrap();
    let notifications = poller.wait(POLL_TIMEOUT).unwrap();
    let elapsed = now.elapsed();
    assert_eq!(1, notifications.len());
    assert_eq!(timer.get_id(), notifications[0].id);
    assert!(elapsed > Duration::from_millis(TIMEOUT as u64));
    assert!(elapsed < Duration::from_millis(POLL_TIMEOUT as u64));
}

#[test]
fn test_set_interval() {
    let mut poller = Poller::new().unwrap();
    let registrar = poller.get_registrar();
    let timer = registrar.set_interval(TIMEOUT).unwrap();
    let now = Instant::now();
    for i in 1..5 {
        let notifications = poller.wait(POLL_TIMEOUT).unwrap();
        timer.arm();
        let elapsed = now.elapsed();
        assert_eq!(1, notifications.len());
        assert_eq!(timer.get_id(), notifications[0].id);
        assert!(elapsed > Duration::from_millis(i*TIMEOUT as u64));
        assert!(elapsed < Duration::from_millis(POLL_TIMEOUT as u64));
    }
    assert!(registrar.cancel_timeout(timer).is_ok());
    let now = Instant::now();
    let notifications = poller.wait(FINAL_POLL_TIMEOUT).unwrap();
    assert!(now.elapsed() > Duration::from_millis(FINAL_POLL_TIMEOUT as u64));
    assert_eq!(0, notifications.len());
}
