use std::collections::BinaryHeap;
use std::cmp::{Ordering, Ord, PartialOrd, PartialEq};
use std::time::{Instant, Duration};
use event::Event;
use notification::Notification;

/// Store timers in a binary heap. Keep them sorted by which timer is going to expire first.
pub struct TimerHeap {
    timers: BinaryHeap<TimerEntry>
}

impl TimerHeap {
    /// Create a new TimerHeap
    pub fn new() -> TimerHeap {
        TimerHeap {
            timers: BinaryHeap::new()
        }
    }

    /// Return the number of timers in the heap
    pub fn len(&self) -> usize {
        self.timers.len()
    }

    /// Insert a TimerEntry into the heap
    pub fn insert(&mut self, entry: TimerEntry) {
        self.timers.push(entry);
    }

    /// Remove a TimerEnry by Id
    ///
    /// Return the entry if it exists, None otherwise
    ///
    /// Note, in a large heap this is probably expensive.
    pub fn remove(&mut self, id: usize) -> Option<TimerEntry> {
        let mut popped = Vec::with_capacity(self.timers.len());
        while let Some(entry) = self.timers.pop() {
            if entry.id == id {
                self.timers.extend(popped);
                return Some(entry);
            } else {
                popped.push(entry);
            }
        }
        self.timers.extend(popped);
        None
    }

    /// Return the amount of time remaining (in ms) for the earliest expiring timer
    /// Return `None` if there are no timers in the heap
    pub fn time_remaining(&self) -> Option<u64> {
        self._time_remaining(Instant::now())
    }

    /// A deterministically testable version of `time_remaining()`
    fn _time_remaining(&self, now: Instant) -> Option<u64> {
        self.timers.peek().map(|e| {
            if now > e.expires_at {
                return 0;
            }
            let duration = e.expires_at - now;
            // We add a millisecond if there is a fractional ms milliseconds in
            // duration.subsec_nanos() / 1000000 so that we never fire early.
            let nanos = duration.subsec_nanos() as u64;
            // TODO: This can almost certainly be done faster
            let subsec_ms = nanos / 1000000;
            let mut remaining = duration.as_secs()*1000 + subsec_ms;
            if subsec_ms * 1000000 < nanos {
                remaining += 1;
            }
            remaining
        })
    }

    /// Return the earliest timeout based on a user timeout and the least remaining time in the
    /// next timer to fire.
    pub fn earliest_timeout(&self, user_timeout_ms: usize) -> usize {
        if let Some(remaining) = self.time_remaining() {
            println!("TIME REMAINING = {:?}", remaining);
            if user_timeout_ms < remaining as usize {
                user_timeout_ms
            } else {
                remaining as usize
            }
        } else {
            user_timeout_ms
        }
    }

    /// Return all expired timer ids as Read notifications
    ///
    /// Any recurring timers will be re-added to the heap in the correct spot
    pub fn expired(&mut self) -> Vec<Notification> {
        self._expired(Instant::now())
    }

    /// A deterministically testable version of `expired()`
    pub fn _expired(&mut self, now: Instant) -> Vec<Notification> {
        let mut expired = Vec::new();
        while let Some(mut popped) = self.timers.pop() {
            if popped.expires_at <= now {
                expired.push(Notification {id: popped.id, event: Event::Read});
                if popped.recurring {
                    // We use the expired_at time so we don't keep skewing later and later
                    // by adding the duration to the current time.
                    popped.expires_at += popped.duration;
                    self.timers.push(popped);
                }
            } else {
                self.timers.push(popped);
                return expired;
            }
        }
        expired
    }
}

#[derive(Eq, Debug)]
pub struct TimerEntry {
    recurring: bool,
    duration: Duration,
    expires_at: Instant,
    id: usize
}

impl TimerEntry {
    pub fn new(id: usize, duration_ms: u64, recurring: bool) -> TimerEntry {
        let duration = Duration::from_millis(duration_ms);
        TimerEntry {
            recurring: recurring,
            duration: duration,
            expires_at: Instant::now() + duration,
            id: id
        }
    }
}

impl Ord for TimerEntry {
    // Order the times backwards because we are sorting them via a max heap
    fn cmp(&self, other: &TimerEntry) -> Ordering {
        if self.expires_at > other.expires_at {
            return Ordering::Less;
        }
        if self.expires_at < other.expires_at {
            return Ordering::Greater;
        }
        Ordering::Equal
    }
}

impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &TimerEntry) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for TimerEntry {
    fn eq(&self, other: &TimerEntry) -> bool {
        self.expires_at == other.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::{TimerHeap, TimerEntry};
    use std::time::{Instant, Duration};

    #[test]
    fn time_remaining() {
        let mut heap = TimerHeap::new();
        let now = Instant::now();
        let duration = Duration::from_millis(500);
        let entry = TimerEntry {
            id: 1,
            recurring: false,
            duration: duration,
            expires_at: now + duration
        };
        heap.insert(entry);
        assert_matches!(heap._time_remaining(now), Some(500));
        assert_matches!(heap._time_remaining(now + duration), Some(0));
        assert_matches!(heap._time_remaining(now + duration + Duration::from_millis(100)),
                        Some(0));
        assert_matches!(heap.remove(2), None);
        let entry = heap.remove(1).unwrap();
        assert_eq!(entry.id, 1);
        assert_matches!(heap._time_remaining(now), None);
    }

    #[test]
    fn expired_non_recurring() {
        let mut heap = TimerHeap::new();
        let now = Instant::now();
        let duration = Duration::from_millis(500);
        let entry = TimerEntry {
            id: 1,
            recurring: false,
            duration: duration,
            expires_at: now + duration
        };
        heap.insert(entry);
        assert_eq!(heap._expired(now), vec![]);
        let v = heap._expired(now + duration);
        assert_eq!(v.len(), 1);
        assert_eq!(heap.len(), 0);
        assert_eq!(heap._expired(now + duration), vec![]);
    }

    #[test]
    fn expired_recurring() {
        let mut heap = TimerHeap::new();
        let now = Instant::now();
        let duration = Duration::from_millis(500);
        let entry = TimerEntry {
            id: 1,
            recurring: true,
            duration: duration,
            expires_at: now + duration
        };
        heap.insert(entry);
        assert_eq!(heap._expired(now), vec![]);
        let v = heap._expired(now + duration);
        assert_eq!(v.len(), 1);
        assert_eq!(heap.len(), 1);
        assert_eq!(heap._expired(now + duration + Duration::from_millis(1)), vec![]);
        let v = heap._expired(now + duration + duration);
        assert_eq!(v.len(), 1);
        assert_eq!(heap.len(), 1);
        assert_eq!(heap._expired(now + duration + duration), vec![]);
    }
}
