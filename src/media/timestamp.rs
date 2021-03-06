use std::{
    cmp::Ordering,
    fmt,
    ops::{Add, Sub},
};

use crate::metadata::{Duration, Timestamp4Humans};

#[derive(Clone, Copy, Default, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Timestamp(u64);

impl Timestamp {
    pub fn new(value: u64) -> Self {
        Timestamp(value)
    }

    pub fn for_humans(self) -> Timestamp4Humans {
        Timestamp4Humans::from_nano(self.0)
    }

    pub fn as_f64(self) -> f64 {
        self.0 as f64
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn saturating_sub(self, rhs: Duration) -> Self {
        Timestamp(self.0.saturating_sub(rhs.as_u64()))
    }
}

impl From<u64> for Timestamp {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<i64> for Timestamp {
    fn from(value: i64) -> Self {
        Self(value as u64)
    }
}

impl From<Duration> for Timestamp {
    fn from(duration: Duration) -> Self {
        Self(duration.as_u64())
    }
}

impl Sub for Timestamp {
    type Output = Duration;

    fn sub(self, rhs: Timestamp) -> Duration {
        Duration::from_nanos(self.0 - rhs.0)
    }
}

impl Add<Duration> for Timestamp {
    type Output = Timestamp;

    fn add(self, rhs: Duration) -> Timestamp {
        Timestamp(self.0 + rhs.as_u64())
    }
}

impl Sub<Duration> for Timestamp {
    type Output = Timestamp;

    fn sub(self, rhs: Duration) -> Timestamp {
        Timestamp(self.0 - rhs.as_u64())
    }
}

impl PartialOrd<Duration> for Timestamp {
    fn partial_cmp(&self, rhs: &Duration) -> Option<Ordering> {
        Some(self.0.cmp(&rhs.as_u64()))
    }
}

impl PartialEq<Duration> for Timestamp {
    fn eq(&self, rhs: &Duration) -> bool {
        self.0 == rhs.as_u64()
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ts {}", self.0)
    }
}
