use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn unix_seconds() -> u64 {
    seconds_since_epoch(SystemTime::now())
}

fn seconds_since_epoch(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn persisted_time_uses_whole_unix_seconds_and_zero_before_the_epoch() {
        assert_eq!(seconds_since_epoch(UNIX_EPOCH), 0);
        assert_eq!(
            seconds_since_epoch(UNIX_EPOCH + Duration::from_millis(1_700_000_123_987)),
            1_700_000_123
        );
        assert_eq!(seconds_since_epoch(UNIX_EPOCH - Duration::from_secs(1)), 0);
    }

    #[test]
    fn current_time_stays_in_unix_seconds() {
        let before = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let actual = unix_seconds();
        let after = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        assert!((before..=after).contains(&actual));
    }
}
