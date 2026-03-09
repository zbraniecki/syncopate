use crate::clock::{MonoInstant, WallInstant};
use crate::task::Drift;
use std::time::Duration;

pub(crate) fn calculate_drift(now: MonoInstant, deadline: MonoInstant) -> Drift {
    if now > deadline {
        Drift::Late(now - deadline)
    } else if now < deadline {
        Drift::Early(deadline - now)
    } else {
        Drift::OnTime
    }
}

pub(crate) fn next_absolute_deadline(
    wall_now: WallInstant,
    period: Duration,
    offset_dur: Duration,
    last_wall_deadline: Option<WallInstant>,
) -> WallInstant {
    match floor_wall_deadline(wall_now, period, offset_dur) {
        Some(current) if last_wall_deadline.is_some_and(|last| last >= current) => {
            last_wall_deadline.unwrap() + period
        }
        Some(current) => {
            if let Some(last) = last_wall_deadline {
                let next_after_last = last + period;
                if next_after_last < current {
                    return next_after_last;
                }
            }
            current
        }
        None => match last_wall_deadline {
            Some(last) if last >= WallInstant(offset_dur.as_nanos() as u64) => last + period,
            _ => WallInstant(offset_dur.as_nanos() as u64),
        },
    }
}

pub(crate) fn floor_wall_deadline(
    wall_now: WallInstant,
    period: Duration,
    offset: Duration,
) -> Option<WallInstant> {
    let now_nanos = wall_now.as_nanos();
    let offset_nanos = offset.as_nanos() as u64;

    if now_nanos < offset_nanos {
        return None;
    }

    let period_nanos = period.as_nanos() as u64;
    let aligned = ((now_nanos - offset_nanos) / period_nanos) * period_nanos + offset_nanos;
    Some(WallInstant(aligned))
}

pub(crate) fn missed_offsets(elapsed_ns: u128, period_ns: u128, count: u32) -> Vec<Duration> {
    (0..count)
        .map(|i| {
            let offset = period_ns * i as u128;
            Duration::from_nanos((elapsed_ns - offset) as u64)
        })
        .collect()
}
