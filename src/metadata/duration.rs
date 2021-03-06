use std::{
    fmt,
    ops::{Div, DivAssign, Mul, MulAssign},
};

// FIXME: consider moving to std::time::Duration when `div_duration` is stabilized.

#[derive(Clone, Copy, Default, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Duration(u64);

impl Duration {
    pub const fn from_nanos(nanos: u64) -> Self {
        Duration(nanos)
    }

    pub const fn from_secs(secs: u64) -> Self {
        Duration(secs * 1_000_000_000u64)
    }

    pub fn as_f64(self) -> f64 {
        self.0 as f64
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn as_i64(self) -> i64 {
        self.0 as i64
    }
}

impl Into<u64> for Duration {
    fn into(self) -> u64 {
        self.0
    }
}

impl Div for Duration {
    type Output = Duration;

    fn div(self, rhs: Duration) -> Self::Output {
        Duration(self.0 / rhs.0)
    }
}

impl Div<u64> for Duration {
    type Output = Duration;

    fn div(self, rhs: u64) -> Self::Output {
        Duration(self.0 / rhs)
    }
}

impl DivAssign<u64> for Duration {
    fn div_assign(&mut self, rhs: u64) {
        *self = Duration(self.0 / rhs);
    }
}

impl Mul<u64> for Duration {
    type Output = Duration;

    fn mul(self, rhs: u64) -> Self::Output {
        Duration(self.0 * rhs)
    }
}

impl MulAssign<u64> for Duration {
    fn mul_assign(&mut self, rhs: u64) {
        *self = Duration(self.0 * rhs);
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "idx range {}", self.0)
    }
}
