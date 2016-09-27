/// Test Channels where the receiver is pollable

extern crate amy;

use amy::{Poller, Event};

#[test]
fn send_wakes_poller() {
    let mut poller = Poller::new().unwrap();
    let registrar = poller.get_registrar();
    let (tx, rx) = registrar.channel().unwrap();

    // no notifications if nothing is registered
    let notifications = poller.wait(50).unwrap();
    assert_eq!(0, notifications.len());

    // Send causes the poller to wakeup
    tx.send("a").unwrap();

    // We poll and get a notification that there is something to receive and receive the sent value
    let notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    assert_eq!(rx.get_id(), notifications[0].id);
    assert_eq!(Event::Read, notifications[0].event);
    assert_eq!("a", rx.try_recv().unwrap());
    assert!(rx.try_recv().is_err());
}

#[test]
fn multiple_sends_wake_poller_once() {
    let mut poller = Poller::new().unwrap();
    let registrar = poller.get_registrar();
    let (tx, rx) = registrar.channel().unwrap();

    tx.send("a").unwrap();
    tx.send("b").unwrap();

    let notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    assert_eq!(rx.get_id(), notifications[0].id);
    assert_eq!("a", rx.try_recv().unwrap());
    assert_eq!("b", rx.try_recv().unwrap());

    let notifications = poller.wait(50).unwrap();
    assert_eq!(0, notifications.len());
}

#[test]
fn send_before_poll_and_after_poll_but_before_recv_only_wakes_poller_once() {
    let mut poller = Poller::new().unwrap();
    let registrar = poller.get_registrar();
    let (tx, rx) = registrar.channel().unwrap();

    tx.send("a").unwrap();

    let notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    assert_eq!(rx.get_id(), notifications[0].id);

    // We haven't done a receive yet, so this just increments the atomic counter instead of
    // triggering the poll wakeup. This is an optimization since atomic counters are much cheaper
    // than syscalls.
    tx.send("b").unwrap();

    assert_eq!("a", rx.try_recv().unwrap());
    assert_eq!("b", rx.try_recv().unwrap());

    let notifications = poller.wait(50).unwrap();
    assert_eq!(0, notifications.len());
}

#[test]
fn send_after_receive_after_poll_followed_by_recv_wakes_poller_again() {
    let mut poller = Poller::new().unwrap();
    let registrar = poller.get_registrar();
    let (tx, rx) = registrar.channel().unwrap();

    tx.send("a").unwrap();

    let notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    assert_eq!(rx.get_id(), notifications[0].id);
    assert_eq!("a", rx.try_recv().unwrap());

    // At this point we did a receive so the atomic pending counter is reduced to zero again. Any
    // new send will trigger a state change on the file descriptor. This will cause the poller to
    // wakeup. A single recv will just decrement the counter, but not call a subsequent clear,
    // so the receiver will remain readable and wake the poller. Clear is only called when
    // rx.try_recv() is called one more time.

    tx.send("b").unwrap();
    assert_eq!("b", rx.try_recv().unwrap());
    let notifications = poller.wait(1000).unwrap();
    assert_eq!(1, notifications.len());
    assert_eq!(rx.get_id(), notifications[0].id);
    assert!(rx.try_recv().is_err());
}

#[test]
fn send_after_receive_after_poll_followed_by_recv_until_err_doesnt_wake_polller_again() {
    let mut poller = Poller::new().unwrap();
    let registrar = poller.get_registrar();
    let (tx, rx) = registrar.channel().unwrap();

    tx.send("a").unwrap();

    let notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    assert_eq!(rx.get_id(), notifications[0].id);
    assert_eq!("a", rx.try_recv().unwrap());

    // At this point we did a receive so the atomic pending counter is reduced to zero again. Any
    // new send will trigger a state change on the file descriptor. This will cause the poller to
    // wakeup. However, if we do another receive, there will be nothing to receive since the
    // counter is 0. This will result in a clear on the file descriptor which will make it no
    // longer readable and therefore not wake the poller.

    tx.send("b").unwrap();
    assert_eq!("b", rx.try_recv().unwrap());
    assert!(rx.try_recv().is_err());
    let notifications = poller.wait(50).unwrap();
    assert_eq!(0, notifications.len());
}

#[test]
/// Ensure that when the user event is cleared that retriggering it wakes the poller
fn send_poll_receive_twice_then_send_poll_receive_once() {
    let mut poller = Poller::new().unwrap();
    let registrar = poller.get_registrar();
    let (tx, rx) = registrar.channel().unwrap();

    tx.send("a").unwrap();

    let notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    assert_eq!(rx.get_id(), notifications[0].id);
    assert_eq!("a", rx.try_recv().unwrap());
    assert!(rx.try_recv().is_err());

    tx.send("b").unwrap();

    let notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    assert_eq!(rx.get_id(), notifications[0].id);
    assert_eq!("b", rx.try_recv().unwrap());
}

#[test]
fn simple_sync_channel_test() {
    let mut poller = Poller::new().unwrap();
    let registrar = poller.get_registrar();
    let (tx, rx) = registrar.sync_channel(1).unwrap();

    // no notifications if nothing is registered
    let notifications = poller.wait(50).unwrap();
    assert_eq!(0, notifications.len());

    // Send causes the poller to wakeup
    tx.send("a").unwrap();

    // We poll and get a notification that there is something to receive and receive the sent value
    let notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    assert_eq!(rx.get_id(), notifications[0].id);
    assert_eq!(Event::Read, notifications[0].event);

    // Send should fail because buffer is of size 1
    tx.try_send("b").is_err();

    assert_eq!("a", rx.try_recv().unwrap());
    assert!(rx.try_recv().is_err());

    // Send should succeed because we received the previous value
    tx.try_send("b").unwrap();
    assert_eq!("b", rx.try_recv().unwrap());
}
