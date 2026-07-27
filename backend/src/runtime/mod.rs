mod watchdog;

/* [277A-6] Watchdog doble señal: heartbeat + HTTP probe loopback */
pub use watchdog::{
    spawn_runtime_watchdog, HttpProbeConfig, RuntimeHeartbeat, RuntimeWatchdogConfig,
};
