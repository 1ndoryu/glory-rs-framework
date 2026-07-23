use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default)]
pub struct RuntimeHeartbeat {
    sequence: Arc<AtomicU64>,
}

impl RuntimeHeartbeat {
    fn pulse(&self) {
        self.sequence.fetch_add(1, Ordering::Relaxed);
    }

    #[must_use]
    pub fn sequence(&self) -> u64 {
        self.sequence.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RuntimeWatchdogConfig {
    pub pulse_interval: Duration,
    pub check_interval: Duration,
    pub freeze_after: Duration,
}

impl Default for RuntimeWatchdogConfig {
    fn default() -> Self {
        Self {
            pulse_interval: Duration::from_secs(5),
            check_interval: Duration::from_secs(5),
            freeze_after: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WatchdogDecision {
    AwaitingFirstPulse,
    Progressed,
    Healthy,
    Frozen,
}

fn decide_watchdog_state(
    last_sequence: u64,
    current_sequence: u64,
    unchanged_for: Duration,
    freeze_after: Duration,
) -> WatchdogDecision {
    if current_sequence == 0 {
        return WatchdogDecision::AwaitingFirstPulse;
    }
    if current_sequence != last_sequence {
        return WatchdogDecision::Progressed;
    }
    if unchanged_for >= freeze_after {
        return WatchdogDecision::Frozen;
    }
    WatchdogDecision::Healthy
}

fn monitor_gap_is_ambiguous(check_gap: Duration, freeze_after: Duration) -> bool {
    check_gap >= freeze_after
}

/* [237A-4] El watchdog usa una secuencia y `Instant`, no timestamps Unix.
 * Un runtime que todavía no produjo su primer pulso permanece en `Starting`;
 * solo una secuencia previamente válida y estancada puede activar recovery.
 * Si el propio hilo monitor quedó suspendido, reinicia su baseline porque no
 * puede distinguir suspensión del host de starvation exclusivo de Tokio. */
pub fn spawn_runtime_watchdog<F>(
    config: RuntimeWatchdogConfig,
    on_freeze: F,
) -> std::io::Result<RuntimeHeartbeat>
where
    F: FnOnce() + Send + 'static,
{
    assert!(
        !config.pulse_interval.is_zero(),
        "pulse_interval must be positive"
    );
    assert!(
        !config.check_interval.is_zero(),
        "check_interval must be positive"
    );
    assert!(
        !config.freeze_after.is_zero(),
        "freeze_after must be positive"
    );

    let heartbeat = RuntimeHeartbeat::default();
    let monitored_heartbeat = heartbeat.clone();

    std::thread::Builder::new()
        .name("rt-watchdog".into())
        .spawn(move || {
            let mut last_sequence = 0;
            let mut last_progress = Instant::now();
            let mut last_check = Instant::now();
            loop {
                std::thread::sleep(config.check_interval);
                let current_sequence = monitored_heartbeat.sequence();
                let checked_at = Instant::now();
                let check_gap = checked_at.duration_since(last_check);
                last_check = checked_at;
                if monitor_gap_is_ambiguous(check_gap, config.freeze_after) {
                    eprintln!(
                        "[rt-watchdog] monitor suspendido durante {}s; baseline reiniciado",
                        check_gap.as_secs()
                    );
                    last_sequence = current_sequence;
                    last_progress = checked_at;
                    continue;
                }
                match decide_watchdog_state(
                    last_sequence,
                    current_sequence,
                    last_progress.elapsed(),
                    config.freeze_after,
                ) {
                    WatchdogDecision::Progressed => {
                        last_sequence = current_sequence;
                        last_progress = Instant::now();
                    }
                    WatchdogDecision::AwaitingFirstPulse | WatchdogDecision::Healthy => {}
                    WatchdogDecision::Frozen => {
                        on_freeze();
                        break;
                    }
                }
            }
        })?;

    let pulsing_heartbeat = heartbeat.clone();
    tokio::spawn(async move {
        pulsing_heartbeat.pulse();
        loop {
            tokio::time::sleep(config.pulse_interval).await;
            pulsing_heartbeat.pulse();
        }
    });

    Ok(heartbeat)
}

#[cfg(test)]
mod tests {
    use super::{decide_watchdog_state, monitor_gap_is_ambiguous, WatchdogDecision};
    use std::time::Duration;

    const FREEZE_AFTER: Duration = Duration::from_secs(30);

    #[test]
    fn zero_sequence_never_reports_freeze() {
        assert_eq!(
            decide_watchdog_state(0, 0, Duration::from_secs(3_600), FREEZE_AFTER),
            WatchdogDecision::AwaitingFirstPulse
        );
    }

    #[test]
    fn first_pulse_establishes_progress() {
        assert_eq!(
            decide_watchdog_state(0, 1, Duration::from_secs(60), FREEZE_AFTER),
            WatchdogDecision::Progressed
        );
    }

    #[test]
    fn advancing_sequence_resets_progress() {
        assert_eq!(
            decide_watchdog_state(4, 5, Duration::from_secs(60), FREEZE_AFTER),
            WatchdogDecision::Progressed
        );
    }

    #[test]
    fn valid_stalled_sequence_reports_freeze() {
        assert_eq!(
            decide_watchdog_state(5, 5, FREEZE_AFTER, FREEZE_AFTER),
            WatchdogDecision::Frozen
        );
    }

    #[test]
    fn recent_valid_sequence_remains_healthy() {
        assert_eq!(
            decide_watchdog_state(5, 5, Duration::from_secs(29), FREEZE_AFTER),
            WatchdogDecision::Healthy
        );
    }

    #[test]
    fn suspended_monitor_is_ambiguous_instead_of_frozen() {
        assert!(monitor_gap_is_ambiguous(FREEZE_AFTER, FREEZE_AFTER));
        assert!(!monitor_gap_is_ambiguous(
            Duration::from_secs(29),
            FREEZE_AFTER
        ));
    }
}
