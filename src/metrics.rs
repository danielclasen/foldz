//! Passes the engine's heartbeat and counters to the ops listener: the RT
//! thread's heartbeat drives `/healthz`, and the counters are rendered on
//! `/metrics`.

use std::time::Duration;

use samplez::Engine;
use samplez_service::{Counter, Metrics, Ops};

use crate::api::{Counters, EngineSlot};

/// How often [`Exporter::run`] samples the engine.
pub const PERIOD: Duration = Duration::from_millis(100);

/// Reads one counter's value since start.
type Value = fn(&Counters) -> u64;

/// The engine counters exported, by metric name: the value since start
/// and the help text.
const ENGINE: [(&str, Value, &str); 13] = [
    (
        "samplez_rt_heartbeat_blocks_total",
        |c| c.heartbeat_blocks,
        "Blocks the RT thread processed.",
    ),
    (
        "samplez_deadline_miss_total",
        |c| c.deadline_miss,
        "Blocks committed later than their deadline plus a quarter block period.",
    ),
    (
        "samplez_overrun_skipped_samples_total",
        |c| c.overrun_skipped_samples,
        "Samples skipped while more than maxCatchupBlocks behind; libmxl zero-fills them.",
    ),
    (
        "samplez_wait_early_total",
        |c| c.wait_early,
        "Blocks processed before their deadline.",
    ),
    (
        "samplez_input_wait_timeout_total",
        |c| c.input_wait_timeout,
        "Input waits that reached the block deadline.",
    ),
    (
        "samplez_input_late_blocks_total",
        |c| c.input_late_blocks,
        "Input blocks not read because the producer's head was below them.",
    ),
    (
        "samplez_input_silenced_blocks_total",
        |c| c.input_silenced_blocks,
        "Input blocks silenced for any reason.",
    ),
    (
        "samplez_input_probes_total",
        |c| c.input_probes,
        "Validity probes of failed inputs.",
    ),
    (
        "samplez_clock_steps_total",
        |c| c.clock_steps,
        "Steps of libmxl's clock against CLOCK_MONOTONIC.",
    ),
    (
        "samplez_output_held_blocks_total",
        |c| c.output_held_blocks,
        "Output blocks below the last commit, not written.",
    ),
    (
        "samplez_output_failed_writes_total",
        |c| c.output_failed_writes,
        "Output open or commit calls libmxl refused.",
    ),
    (
        "samplez_telemetry_dropped_total",
        |c| c.telemetry_dropped,
        "RT telemetry records dropped on a full ring.",
    ),
    (
        "samplez_rt_minor_faults_total",
        |c| c.rt_minor_faults,
        "Minor page faults of the RT thread.",
    ),
];

/// One exported counter and the engine value it last added up to.
struct Exported {
    counter: Counter,
    value: Value,
    last: u64,
}

pub struct Exporter {
    ops: Ops,
    engine: EngineSlot,
    exported: Vec<Exported>,
    heartbeat: u64,
}

impl Exporter {
    pub fn new(ops: Ops, engine: EngineSlot) -> Exporter {
        let metrics: &Metrics = ops.metrics();
        let exported = ENGINE
            .iter()
            .map(|(name, value, help)| Exported {
                counter: metrics.counter(name, help, &[]),
                value: *value,
                last: 0,
            })
            .collect();
        Exporter {
            ops,
            engine,
            exported,
            heartbeat: 0,
        }
    }

    /// Beats the ops heartbeat when the RT thread's has advanced since the
    /// last call, and adds what each engine counter gained to its metric.
    pub fn sample(&mut self) {
        let engine = self.engine.lock().expect("engine lock poisoned");
        let Some(engine): Option<&Engine> = engine.as_ref() else {
            return;
        };
        let c = Counters::of(engine.counters());
        if c.heartbeat_blocks != self.heartbeat {
            self.heartbeat = c.heartbeat_blocks;
            self.ops.heartbeat().beat();
        }
        for e in &mut self.exported {
            let now = (e.value)(&c);
            e.counter.add(now.saturating_sub(e.last));
            e.last = now;
        }
    }

    /// Samples every [`PERIOD`]; never returns.
    pub async fn run(mut self) {
        let mut ticks = tokio::time::interval(PERIOD);
        loop {
            ticks.tick().await;
            self.sample();
        }
    }
}
