use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonoInstant(pub u64);

impl MonoInstant {
    pub const ZERO: Self = Self(0);

    #[inline]
    pub fn as_nanos(self) -> u64 {
        self.0
    }

    #[inline]
    pub fn checked_duration_since(self, earlier: Self) -> Option<Duration> {
        self.0.checked_sub(earlier.0).map(Duration::from_nanos)
    }

    #[inline]
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(earlier.0))
    }
}

impl std::ops::Add<Duration> for MonoInstant {
    type Output = Self;
    fn add(self, rhs: Duration) -> Self {
        Self(self.0.saturating_add(rhs.as_nanos() as u64))
    }
}

impl std::ops::AddAssign<Duration> for MonoInstant {
    fn add_assign(&mut self, rhs: Duration) {
        *self = *self + rhs;
    }
}

impl std::ops::Sub<Duration> for MonoInstant {
    type Output = Self;
    fn sub(self, rhs: Duration) -> Self {
        Self(self.0.saturating_sub(rhs.as_nanos() as u64))
    }
}

impl std::ops::Sub<MonoInstant> for MonoInstant {
    type Output = Duration;
    fn sub(self, rhs: MonoInstant) -> Duration {
        self.saturating_duration_since(rhs)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WallInstant(pub u64);

impl WallInstant {
    pub const UNIX_EPOCH: Self = Self(0);

    #[inline]
    pub fn as_nanos(self) -> u64 {
        self.0
    }

    #[inline]
    pub fn checked_duration_since(self, earlier: Self) -> Option<Duration> {
        self.0.checked_sub(earlier.0).map(Duration::from_nanos)
    }
}

impl std::ops::Add<Duration> for WallInstant {
    type Output = Self;
    fn add(self, rhs: Duration) -> Self {
        Self(self.0.saturating_add(rhs.as_nanos() as u64))
    }
}

impl std::ops::AddAssign<Duration> for WallInstant {
    fn add_assign(&mut self, rhs: Duration) {
        *self = *self + rhs;
    }
}

impl std::ops::Sub<Duration> for WallInstant {
    type Output = Self;
    fn sub(self, rhs: Duration) -> Self {
        Self(self.0.saturating_sub(rhs.as_nanos() as u64))
    }
}
