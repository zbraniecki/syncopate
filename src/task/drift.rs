use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drift {
    Early(Duration),
    OnTime,
    Late(Duration),
}

impl Drift {
    pub fn as_nanos_signed(&self) -> i128 {
        match self {
            Drift::Early(d) => -(d.as_nanos() as i128),
            Drift::OnTime => 0,
            Drift::Late(d) => d.as_nanos() as i128,
        }
    }
}

impl std::fmt::Display for Drift {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Drift::Early(d) => write!(f, "-{}", FmtDuration(*d)),
            Drift::OnTime => write!(f, "0ns"),
            Drift::Late(d) => write!(f, "+{}", FmtDuration(*d)),
        }
    }
}

struct FmtDuration(Duration);

impl std::fmt::Display for FmtDuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let nanos = self.0.as_nanos();
        if nanos < 1_000 {
            write!(f, "{}ns", nanos)
        } else if nanos < 1_000_000 {
            write!(f, "{}µs", nanos / 1_000)
        } else if nanos < 1_000_000_000 {
            write!(f, "{}ms", nanos / 1_000_000)
        } else {
            let secs = self.0.as_secs();
            let ms = (nanos % 1_000_000_000) / 1_000_000;
            if ms == 0 {
                write!(f, "{}s", secs)
            } else {
                write!(f, "{}.{:03}s", secs, ms)
            }
        }
    }
}
