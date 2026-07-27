/* [277A-6] Watchdog de runtime con doble señal.
 * Fase 4 del plan-incidente-freeze-bad-gateway-nakomi-2026-07-24.
 *
 * Cambios respecto al watchdog anterior:
 * - Señal A: heartbeat Tokio (existente) — sequence monotónica.
 * - Señal B: HTTP probe loopback (nuevo) — GET localhost:port/healthz.
 * - Solo mata si AMBAS señales fallan durante el umbral (120s por defecto).
 * - Periodo de gracia de arranque (60s): no mata durante el inicio.
 * - Logging detallado antes de exit(1) para diagnóstico post-mortem.
 * - Umbral subido de 30s a 120s para tolerar picos de carga legítimos.
 *
 * Rollback: GLORY_HTTP_WATCHDOG=false desactiva todo el watchdog. */

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

/// Configuración del watchdog de runtime.
/// [277A-6] Añadidos `http_probe` y `grace_period` para doble señal.
#[derive(Clone, Debug)]
pub struct RuntimeWatchdogConfig {
    pub pulse_interval: Duration,
    pub check_interval: Duration,
    pub freeze_after: Duration,
    /// [277A-6] Configuración del probe HTTP loopback. `None` = solo heartbeat.
    pub http_probe: Option<HttpProbeConfig>,
    /// [277A-6] Periodo de gracia tras arrancar: no mata durante este tiempo.
    pub grace_period: Duration,
}

#[derive(Clone, Debug)]
pub struct HttpProbeConfig {
    /// Puerto local del servidor HTTP (ej: 3000).
    pub port: u16,
    /// Ruta del health endpoint (ej: `/healthz`).
    pub path: String,
    /// Timeout individual del probe TCP+HTTP (default 3s).
    pub timeout: Duration,
}

impl Default for RuntimeWatchdogConfig {
    fn default() -> Self {
        Self {
            pulse_interval: Duration::from_secs(5),
            check_interval: Duration::from_secs(5),
            /* [277A-6] Umbral subido de 30s a 120s: tolera picos de carga legítimos. */
            freeze_after: Duration::from_mins(2),
            http_probe: None,
            grace_period: Duration::from_mins(1),
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

/// [277A-6] Resultado del probe HTTP loopback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProbeResult {
    /// El servidor respondió (al menos aceptó la conexión).
    Responded,
    /// No se pudo conectar o timeout.
    Failed,
}

/// [277A-6] Ejecuta un probe HTTP `GET` mínimo al servidor local.
/// Usa `TcpStream` + HTTP/1.1 manual para no requerir `reqwest` en el framework.
fn http_probe_loopback(port: u16, path: &str, timeout: Duration) -> ProbeResult {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let addr = std::net::SocketAddr::new(
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        port,
    );
    let Ok(stream) = TcpStream::connect_timeout(&addr, timeout) else {
        return ProbeResult::Failed;
    };

    if stream.set_read_timeout(Some(timeout)).is_err()
        || stream.set_write_timeout(Some(timeout)).is_err()
    {
        return ProbeResult::Failed;
    }

    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    let mut stream = stream;
    if stream.write_all(request.as_bytes()).is_err() {
        return ProbeResult::Failed;
    }

    let mut buf = [0u8; 1];
    match stream.read(&mut buf) {
        Ok(n) if n > 0 => ProbeResult::Responded,
        _ => ProbeResult::Failed,
    }
}

/// [277A-6] Estado de diagnóstico para logging antes de exit.
struct DiagnosticState {
    last_sequence: u64,
    current_sequence: u64,
    frozen_for: Duration,
    uptime: Instant,
    probe_url: Option<String>,
    last_probe: Option<ProbeResult>,
}

fn log_diagnostics(diag: &DiagnosticState) {
    eprintln!("[rt-watchdog] RUNTIME FREEZE DETECTED (doble senal)");
    eprintln!(
        "[rt-watchdog] Heartbeat: last_seq={}, current_seq={}, stalled_for={}s",
        diag.last_sequence,
        diag.current_sequence,
        diag.frozen_for.as_secs()
    );
    if let Some(url) = &diag.probe_url {
        let probe_status = match diag.last_probe {
            Some(ProbeResult::Responded) => "responded (unexpected)",
            Some(ProbeResult::Failed) => "failed",
            None => "not checked",
        };
        eprintln!("[rt-watchdog] HTTP probe: {url} -> {probe_status}");
    }
    eprintln!(
        "[rt-watchdog] Uptime: {}s",
        diag.uptime.elapsed().as_secs()
    );
}

/// [277A-6] Estado mutable del loop de monitoreo.
struct MonitorState {
    last_sequence: u64,
    last_progress: Instant,
    last_check: Instant,
    started_at: Instant,
    consecutive_stalled: u32,
}

/// [277A-6] Ejecuta el loop de monitoreo del watchdog.
/// Separado de `spawn_runtime_watchdog` para cumplir límite de 100 líneas.
fn run_monitor_loop<F>(
    config: &RuntimeWatchdogConfig,
    monitored_heartbeat: &RuntimeHeartbeat,
    probe_config: Option<&HttpProbeConfig>,
    on_freeze: F,
) where
    F: FnOnce(),
{
    let mut state = MonitorState {
        last_sequence: 0,
        last_progress: Instant::now(),
        last_check: Instant::now(),
        started_at: Instant::now(),
        consecutive_stalled: 0,
    };

    loop {
        std::thread::sleep(config.check_interval);
        let now = Instant::now();
        let check_gap = now.duration_since(state.last_check);
        state.last_check = now;

        /* Grace period: no evaluar durante los primeros segundos. */
        if now.duration_since(state.started_at) < config.grace_period {
            let current_seq = monitored_heartbeat.sequence();
            if current_seq != 0 {
                state.last_sequence = current_seq;
                state.last_progress = now;
            }
            continue;
        }

        if monitor_gap_is_ambiguous(check_gap, config.freeze_after) {
            eprintln!(
                "[rt-watchdog] monitor suspendido durante {}s; baseline reiniciado",
                check_gap.as_secs()
            );
            state.last_sequence = monitored_heartbeat.sequence();
            state.last_progress = now;
            state.consecutive_stalled = 0;
            continue;
        }

        let current_sequence = monitored_heartbeat.sequence();

        match decide_watchdog_state(
            state.last_sequence,
            current_sequence,
            state.last_progress.elapsed(),
            config.freeze_after,
        ) {
            WatchdogDecision::Progressed => {
                state.last_sequence = current_sequence;
                state.last_progress = now;
                state.consecutive_stalled = 0;
            }
            WatchdogDecision::AwaitingFirstPulse | WatchdogDecision::Healthy => {
                if current_sequence != state.last_sequence {
                    state.consecutive_stalled = 0;
                }
            }
            WatchdogDecision::Frozen => {
                state.consecutive_stalled += 1;

                /* Doble señal: verificar HTTP probe si está configurado. */
                let probe_failed = if let Some(pc) = probe_config {
                    let result = http_probe_loopback(pc.port, &pc.path, pc.timeout);
                    if result == ProbeResult::Responded {
                        state.consecutive_stalled = 0;
                        state.last_progress = now;
                        false
                    } else {
                        true
                    }
                } else {
                    true
                };

                if probe_failed && state.consecutive_stalled > 0 {
                    let frozen_for = state.last_progress.elapsed();

                    log_diagnostics(&DiagnosticState {
                        last_sequence: state.last_sequence,
                        current_sequence,
                        frozen_for,
                        uptime: state.started_at,
                        probe_url: probe_config
                            .as_ref()
                            .map(|pc| format!("http://127.0.0.1:{}{}", pc.port, pc.path)),
                        last_probe: probe_config.as_ref().map(|_| ProbeResult::Failed),
                    });

                    on_freeze();
                    break;
                }
            }
        }
    }
}

/// Spawns the runtime watchdog with double-signal freeze detection.
///
/// [237A-4] Uses a monotonic sequence and `Instant`, not Unix timestamps.
/// [277A-6] Adds optional HTTP probe loopback as second signal.
/// Only triggers recovery when BOTH heartbeat stalled AND HTTP probe fails.
/// Diagnostics are logged to stderr before calling `on_freeze`.
pub fn spawn_runtime_watchdog<F>(
    config: &RuntimeWatchdogConfig,
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
    let monitored = heartbeat.clone();
    let probe = config.http_probe.clone();
    let cfg = config.clone();

    std::thread::Builder::new()
        .name("rt-watchdog".into())
        .spawn(move || {
            run_monitor_loop(&cfg, &monitored, probe.as_ref(), on_freeze);
        })?;

    let pulsing = heartbeat.clone();
    let pulse_interval = config.pulse_interval;
    tokio::spawn(async move {
        pulsing.pulse();
        loop {
            tokio::time::sleep(pulse_interval).await;
            pulsing.pulse();
        }
    });

    Ok(heartbeat)
}

#[cfg(test)]
mod tests {
    use super::{decide_watchdog_state, http_probe_loopback, monitor_gap_is_ambiguous, ProbeResult, WatchdogDecision};
    use std::time::Duration;

    const FREEZE_AFTER: Duration = Duration::from_mins(2);

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
            decide_watchdog_state(5, 5, Duration::from_secs(119), FREEZE_AFTER),
            WatchdogDecision::Healthy
        );
    }

    #[test]
    fn suspended_monitor_is_ambiguous_instead_of_frozen() {
        assert!(monitor_gap_is_ambiguous(FREEZE_AFTER, FREEZE_AFTER));
        assert!(!monitor_gap_is_ambiguous(
            Duration::from_secs(119),
            FREEZE_AFTER
        ));
    }

    #[test]
    fn freeze_threshold_is_120s() {
        assert_eq!(
            decide_watchdog_state(5, 5, Duration::from_secs(30), FREEZE_AFTER),
            WatchdogDecision::Healthy
        );
        assert_eq!(
            decide_watchdog_state(5, 5, Duration::from_secs(120), FREEZE_AFTER),
            WatchdogDecision::Frozen
        );
    }

    #[test]
    fn http_probe_fails_on_unreachable_port() {
        let result = http_probe_loopback(59999, "/healthz", Duration::from_secs(1));
        assert_eq!(result, ProbeResult::Failed);
    }
}
