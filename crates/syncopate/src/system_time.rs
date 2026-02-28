use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ── MonoInstant ──────────────────────────────────────────────────────────────

/// A monotonic timestamp, in nanoseconds since an arbitrary epoch (clock
/// creation for `RealClock`, or zero for `SimClock`).
///
/// Guaranteed never to decrease within a single run. Used for all internal
/// deadline ordering and relative-task scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonoInstant(pub u64);

impl MonoInstant {
    pub const ZERO: Self = Self(0);

    /// Nanoseconds since this clock's epoch. Useful for cheap arithmetic.
    #[inline]
    pub fn as_nanos(self) -> u64 {
        self.0
    }

    /// Returns `Some(duration)` if `self >= earlier`, `None` if the clock
    /// somehow went backward (should never happen with a correct `Clock` impl).
    #[inline]
    pub fn checked_duration_since(self, earlier: Self) -> Option<Duration> {
        self.0.checked_sub(earlier.0).map(Duration::from_nanos)
    }

    /// Like `checked_duration_since` but returns `Duration::ZERO` on underflow.
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
    /// Saturates to `Duration::ZERO` if `rhs > self`.
    fn sub(self, rhs: MonoInstant) -> Duration {
        self.saturating_duration_since(rhs)
    }
}

// ── WallInstant ──────────────────────────────────────────────────────────────

/// A wall-clock timestamp, in nanoseconds since the UNIX epoch.
///
/// Unlike `MonoInstant`, this value *can* jump forward or backward due to NTP
/// corrections, manual adjustments, DST, or suspend/resume. Used only for
/// anchored tasks ("fire at 8 pm") and clock-jump detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WallInstant(pub u64);

impl WallInstant {
    /// Corresponds to `std::time::UNIX_EPOCH`.
    pub const UNIX_EPOCH: Self = Self(0);

    /// Nanoseconds since the UNIX epoch.
    #[inline]
    pub fn as_nanos(self) -> u64 {
        self.0
    }

    /// Returns `Some(duration)` if `self >= earlier`, `None` if the wall
    /// clock went backward (NTP step-back, manual change, etc.).
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

// ── Clock trait ──────────────────────────────────────────────────────────────

/// The scheduler's only interface to time.
///
/// Provides both a monotonic and a wall-clock reading. The two readings from a
/// single call are *not* guaranteed to be perfectly simultaneous, but should be
/// "close enough" for the scheduler's clock-jump detection (threshold: ~100 ms).
pub trait Clock {
    fn monotonic_now(&self) -> MonoInstant;
    fn wall_now(&self) -> WallInstant;
}

/// Blanket impl for `Rc<C>`. Lets a single-threaded test share a `SimClock`
/// between itself and the scheduler:
///
/// ```text
/// let clock = Rc::new(SimClock::new());
/// let mut scheduler = Scheduler::new_simulated(Rc::clone(&clock));
/// clock.advance(Duration::from_millis(500)); // still accessible here
/// ```
impl<C: Clock> Clock for Rc<C> {
    fn monotonic_now(&self) -> MonoInstant {
        (**self).monotonic_now()
    }
    fn wall_now(&self) -> WallInstant {
        (**self).wall_now()
    }
}

/// Blanket impl for `Arc<C>`. Lets a multi-threaded or async test share a
/// `SimClock` while keeping `Scheduler: Send`:
///
/// ```text
/// let clock = Arc::new(SimClock::new());
/// let mut scheduler = Scheduler::new_simulated(Arc::clone(&clock));
/// clock.advance(Duration::from_millis(500)); // accessible from any thread
/// ```
impl<C: Clock> Clock for Arc<C> {
    fn monotonic_now(&self) -> MonoInstant {
        (**self).monotonic_now()
    }
    fn wall_now(&self) -> WallInstant {
        (**self).wall_now()
    }
}

// ── RealClock ────────────────────────────────────────────────────────────────

/// Production clock.
///
/// Monotonic time is measured as nanoseconds elapsed since construction (so
/// `MonoInstant::ZERO` corresponds to when `RealClock::new()` was called).
/// Wall-clock time is nanoseconds since the UNIX epoch via `SystemTime::now()`.
pub struct RealClock {
    mono_epoch: Instant,
}

impl RealClock {
    pub fn new() -> Self {
        Self {
            mono_epoch: Instant::now(),
        }
    }
}

impl Default for RealClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for RealClock {
    fn monotonic_now(&self) -> MonoInstant {
        MonoInstant(self.mono_epoch.elapsed().as_nanos() as u64)
    }

    fn wall_now(&self) -> WallInstant {
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_nanos() as u64;
        WallInstant(ns)
    }
}

// ── SimClock ─────────────────────────────────────────────────────────────────

/// Simulated clock for deterministic testing.
///
/// Both clocks start at zero and advance together via `advance()`. The wall
/// clock can be independently shifted via `jump_wall_clock()` to simulate NTP
/// corrections, suspend/resume, or manual time changes — without touching the
/// monotonic clock.
///
/// Uses `AtomicU64` internally so it is `Send + Sync`, meaning it can be
/// wrapped in either `Rc` (single-threaded tests) or `Arc` (multi-threaded /
/// async tests) and shared with the scheduler via the blanket `Clock` impls.
pub struct SimClock {
    mono_ns: AtomicU64,
    wall_ns: AtomicU64,
}

impl SimClock {
    pub fn new() -> Self {
        Self {
            mono_ns: AtomicU64::new(0),
            wall_ns: AtomicU64::new(0),
        }
    }

    /// Advance both clocks by `duration`. Simulates normal passage of time.
    pub fn advance(&self, duration: Duration) {
        let ns = duration.as_nanos() as u64;
        self.mono_ns.fetch_add(ns, Ordering::Relaxed);
        self.wall_ns.fetch_add(ns, Ordering::Relaxed);
    }

    /// Shift only the wall clock by a signed nanosecond delta, leaving the
    /// monotonic clock unchanged. Simulates an NTP correction, DST boundary,
    /// or suspend/resume event.
    pub fn jump_wall_clock(&self, delta_ns: i64) {
        let current = self.wall_ns.load(Ordering::Relaxed) as i64;
        let adjusted = current.saturating_add(delta_ns).max(0) as u64;
        self.wall_ns.store(adjusted, Ordering::Relaxed);
    }
}

impl Default for SimClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SimClock {
    fn monotonic_now(&self) -> MonoInstant {
        MonoInstant(self.mono_ns.load(Ordering::Relaxed))
    }

    fn wall_now(&self) -> WallInstant {
        WallInstant(self.wall_ns.load(Ordering::Relaxed))
    }
}

// ── TimeReference ─────────────────────────────────────────────────────────────

/// A paired `(MonoInstant, WallInstant)` snapshot that establishes a mapping
/// between the two time domains.
///
/// Captured once at scheduler construction. Re-captured after a detected clock
/// jump so that anchored tasks can be re-anchored to the new wall-clock reality
/// without disturbing relative (monotonic-only) tasks.
pub struct TimeReference {
    mono_ref: MonoInstant,
    wall_ref: WallInstant,
}

impl TimeReference {
    pub fn new(mono_ref: MonoInstant, wall_ref: WallInstant) -> Self {
        Self { mono_ref, wall_ref }
    }

    /// Capture a fresh reference pair from a live clock.
    pub fn capture(clock: &dyn Clock) -> Self {
        Self {
            mono_ref: clock.monotonic_now(),
            wall_ref: clock.wall_now(),
        }
    }

    /// Convert a wall-clock instant to its corresponding monotonic instant.
    ///
    /// If `wall` is before `wall_ref` (clock went backward), the resulting
    /// `MonoInstant` will be before `mono_ref`, saturating at `MonoInstant::ZERO`.
    pub fn wall_to_mono(&self, wall: WallInstant) -> MonoInstant {
        match wall.checked_duration_since(self.wall_ref) {
            Some(d) => self.mono_ref + d,
            None => {
                // wall is before the reference point — subtract from mono side
                let d = self
                    .wall_ref
                    .checked_duration_since(wall)
                    .unwrap_or(Duration::ZERO);
                self.mono_ref - d
            }
        }
    }

    /// Convert a monotonic instant to its corresponding wall-clock instant.
    pub fn mono_to_wall(&self, mono: MonoInstant) -> WallInstant {
        match mono.checked_duration_since(self.mono_ref) {
            Some(d) => self.wall_ref + d,
            None => {
                let d = self
                    .mono_ref
                    .checked_duration_since(mono)
                    .unwrap_or(Duration::ZERO);
                self.wall_ref - d
            }
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_clock_starts_at_zero() {
        let clock = SimClock::new();
        assert_eq!(clock.monotonic_now(), MonoInstant::ZERO);
        assert_eq!(clock.wall_now(), WallInstant::UNIX_EPOCH);
    }

    #[test]
    fn sim_clock_advance_moves_both() {
        let clock = SimClock::new();
        clock.advance(Duration::from_secs(1));
        assert_eq!(clock.monotonic_now().as_nanos(), 1_000_000_000);
        assert_eq!(clock.wall_now().as_nanos(), 1_000_000_000);
    }

    #[test]
    fn sim_clock_wall_jump_does_not_move_mono() {
        let clock = SimClock::new();
        clock.advance(Duration::from_secs(10));
        clock.jump_wall_clock(5_000_000_000); // +5s wall jump
        assert_eq!(clock.monotonic_now().as_nanos(), 10_000_000_000);
        assert_eq!(clock.wall_now().as_nanos(), 15_000_000_000);
    }

    #[test]
    fn sim_clock_negative_wall_jump() {
        let clock = SimClock::new();
        clock.advance(Duration::from_secs(10));
        clock.jump_wall_clock(-3_000_000_000); // -3s NTP correction
        assert_eq!(clock.monotonic_now().as_nanos(), 10_000_000_000);
        assert_eq!(clock.wall_now().as_nanos(), 7_000_000_000);
    }

    #[test]
    fn time_reference_round_trip() {
        let clock = SimClock::new();
        clock.advance(Duration::from_secs(5));
        let time_ref = TimeReference::capture(&clock);

        clock.advance(Duration::from_secs(3));
        let mono = clock.monotonic_now();
        let wall = clock.wall_now();

        // mono -> wall -> mono should round-trip
        assert_eq!(time_ref.wall_to_mono(time_ref.mono_to_wall(mono)), mono);
        // wall -> mono -> wall should round-trip
        assert_eq!(time_ref.mono_to_wall(time_ref.wall_to_mono(wall)), wall);
    }

    #[test]
    fn time_reference_after_wall_jump() {
        let clock = SimClock::new();
        clock.advance(Duration::from_secs(10));
        let time_ref = TimeReference::capture(&clock);

        // Simulate a +2s NTP correction without advancing mono
        clock.jump_wall_clock(2_000_000_000);

        // wall_to_mono should reflect the jump
        let wall_after_jump = clock.wall_now(); // 12s wall, 10s mono
        let expected_mono = MonoInstant(12_000_000_000); // mono_ref(10s) + (12s - 10s)
        assert_eq!(time_ref.wall_to_mono(wall_after_jump), expected_mono);
    }

    #[test]
    fn mono_instant_arithmetic() {
        let a = MonoInstant(1_000_000_000);
        let b = a + Duration::from_millis(500);
        assert_eq!(b.as_nanos(), 1_500_000_000);
        assert_eq!((b - a), Duration::from_millis(500));
        assert_eq!(
            b.checked_duration_since(a),
            Some(Duration::from_millis(500))
        );
        assert_eq!(a.checked_duration_since(b), None);
    }

    #[test]
    fn real_clock_monotonic_advances() {
        let clock = RealClock::new();
        let t0 = clock.monotonic_now();
        std::thread::sleep(Duration::from_millis(10));
        let t1 = clock.monotonic_now();
        assert!(t1 > t0);
    }

    #[test]
    fn rc_shared_sim_clock() {
        let clock = Rc::new(SimClock::new());
        let clock2 = Rc::clone(&clock);

        clock.advance(Duration::from_secs(1));
        // both handles see the same state
        assert_eq!(clock2.monotonic_now().as_nanos(), 1_000_000_000);
        assert_eq!(clock2.wall_now().as_nanos(), 1_000_000_000);

        clock2.jump_wall_clock(-250_000_000);
        assert_eq!(clock.wall_now().as_nanos(), 750_000_000);
        assert_eq!(clock.monotonic_now().as_nanos(), 1_000_000_000);
    }

    #[test]
    fn arc_shared_sim_clock() {
        let clock = Arc::new(SimClock::new());
        let clock2 = Arc::clone(&clock);

        clock.advance(Duration::from_secs(1));
        assert_eq!(clock2.monotonic_now().as_nanos(), 1_000_000_000);
        assert_eq!(clock2.wall_now().as_nanos(), 1_000_000_000);

        clock2.jump_wall_clock(-250_000_000);
        assert_eq!(clock.wall_now().as_nanos(), 750_000_000);
        assert_eq!(clock.monotonic_now().as_nanos(), 1_000_000_000);
    }

    #[test]
    fn arc_sim_clock_is_send() {
        // Verify Arc<SimClock> can be sent to another thread (compile-time check).
        let clock = Arc::new(SimClock::new());
        let clock2 = Arc::clone(&clock);
        let handle = std::thread::spawn(move || {
            clock2.advance(Duration::from_secs(1));
        });
        handle.join().unwrap();
        assert_eq!(clock.monotonic_now().as_nanos(), 1_000_000_000);
    }
}
