use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde_crate::Serialize, serde_crate::Deserialize)
)]
#[cfg_attr(feature = "serde", serde(crate = "serde_crate"))]
pub enum PeriodicSchedule {
    #[default]
    FixedRate,
    FixedDelay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde_crate::Serialize, serde_crate::Deserialize)
)]
#[cfg_attr(feature = "serde", serde(crate = "serde_crate"))]
pub enum MissedTickBehavior {
    RunLatest,
    Burst {
        max: Option<u32>,
    },
    #[default]
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde_crate::Serialize, serde_crate::Deserialize)
)]
#[cfg_attr(feature = "serde", serde(crate = "serde_crate"))]
pub enum Repeat {
    Forever,
    Times(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub early: Duration,
    pub late: Duration,
}

impl Window {
    pub const ZERO: Self = Self {
        early: Duration::ZERO,
        late: Duration::ZERO,
    };

    pub const fn new(early: Duration, late: Duration) -> Self {
        Self { early, late }
    }

    pub const fn symmetric(margin: Duration) -> Self {
        Self {
            early: margin,
            late: margin,
        }
    }

    pub const fn is_zero(&self) -> bool {
        self.early.as_nanos() == 0 && self.late.as_nanos() == 0
    }

    pub const fn total(&self) -> Duration {
        Duration::from_nanos(self.early.as_nanos() as u64 + self.late.as_nanos() as u64)
    }
}
