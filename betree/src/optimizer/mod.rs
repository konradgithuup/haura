pub mod config;

use crate::data_management::Dml;
use crate::database::RootDmu;
use crate::storage_pool::StoragePoolLayer;
use crate::storage_pool::{DiskOffset, GlobalDiskId};
use rand::Rng;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

/// Maximum number of devices supported by the 12-bit GlobalDiskId addressing scheme.
// TODO: reduce number for this component because we usually don't need that many
pub const MAX_DEVICES: usize = 4096;

/// Shared weight buffer using a Sequence Lock for wait-free reads on the cache path.
#[derive(Clone)]
pub struct SharedWeights(pub Arc<[AtomicU32; MAX_DEVICES]>);

impl SharedWeights {
    pub fn new() -> Self {
        let initial_bits = 1.0_f32.to_bits();
        let weights = std::array::from_fn(|_| AtomicU32::new(initial_bits));
        Self(Arc::new(weights))
    }

    /// Fetches a weight for a specific device. Used by the Cache Policy.
    pub fn get_weight(&self, id: GlobalDiskId) -> f32 {
        let bits = self.0[id.as_u16() as usize].load(Ordering::Acquire);
        f32::from_bits(bits)
    }
}

pub struct OptimizerState {
    temperature: f32,
    prev_latency: f64,
    best_latency: f64,
    current_weights: [f32; MAX_DEVICES],
    prev_weights: [f32; MAX_DEVICES],
    best_weights: [f32; MAX_DEVICES],
    // Storage for previous vdev counters to calculate delta latency
    // Index matches GlobalDiskId
    prev_stats_nanos: [u64; MAX_DEVICES],
    prev_stats_count: [u64; MAX_DEVICES],
    active_devices: Vec<GlobalDiskId>,
}

impl OptimizerState {
    pub fn new(config: &config::OptimizerConfig) -> Self {
        Self {
            temperature: config.initial_temperature,
            prev_latency: f64::MAX,
            best_latency: f64::MAX,
            current_weights: [1.0; MAX_DEVICES],
            prev_weights: [1.0; MAX_DEVICES],
            best_weights: [1.0; MAX_DEVICES],
            prev_stats_nanos: [0; MAX_DEVICES],
            prev_stats_count: [0; MAX_DEVICES],
            active_devices: Vec::with_capacity(16),
        }
    }

    fn calculate_epoch_latency(&mut self, dmu: &Arc<RootDmu>) -> f64 {
        let metrics = dmu.spl().metrics();
        let mut total_nanos = 0;
        let mut total_count = 0;
        self.active_devices.clear();

        for (tier_idx, tier) in metrics.tiers.iter().enumerate() {
            if let Some(tier_metrics) = tier {
                for (vdev_idx, stats) in tier_metrics.vdevs.iter().enumerate() {
                    let g_id = DiskOffset::construct_disk_id(tier_idx as u8, vdev_idx as u16);
                    let idx = g_id.as_u16() as usize;
                    self.active_devices.push(g_id);

                    let current_nanos =
                        stats.read_latency_total_nanos + stats.written_latency_total_nanos;
                    let current_count = stats.read_count + stats.written_count;

                    let delta_nanos = current_nanos.saturating_sub(self.prev_stats_nanos[idx]);
                    let delta_count = current_count.saturating_sub(self.prev_stats_count[idx]);

                    if delta_count > 0 {
                        total_nanos += delta_nanos;
                        total_count += delta_count;
                    }

                    self.prev_stats_nanos[idx] = current_nanos;
                    self.prev_stats_count[idx] = current_count;
                }
            }
        }

        if total_count == 0 {
            0.0
        } else {
            total_nanos as f64 / total_count as f64
        }
    }

    fn mutate(&mut self, source: &[f32; MAX_DEVICES], spread: f32) {
        let mut rng = rand::thread_rng();
        for id in &self.active_devices {
            let i = id.as_u16() as usize;
            let delta = rng.gen_range(-spread..spread);
            self.current_weights[i] = (source[i] + delta).clamp(0.1, 10.0);
        }
    }
}

pub fn run_optimizer(
    dmu: Arc<RootDmu>,
    shared: SharedWeights,
    config: config::OptimizerConfig,
    rx: crossbeam_channel::Receiver<()>
) {
    let mut state = OptimizerState::new(&config);
    let mut rng = rand::thread_rng();

    while rx.recv().is_ok() {
        let latency = state.calculate_epoch_latency(&dmu);
        if latency == 0.0 {
            continue;
        } // No IO this epoch

        if latency < state.best_latency {
            state.best_latency = latency;
            state.best_weights = state.current_weights;
        }

        let abs_change = (latency - state.prev_latency).abs();

        if abs_change < config.min_change && state.temperature < config.min_temperature {
            log::debug!(
                "Optimizer: Hibernating (latency: {:.2}ns, temp: {:.2})",
                latency,
                state.temperature
            );
            state.current_weights = state.best_weights;
            state.prev_latency = state.best_latency;
        } else if abs_change > config.max_idle && state.temperature < config.min_temperature {
            log::debug!(
                "Optimizer: Wake-up triggered by latency spike ({:.2}ns)",
                latency
            );
            state.prev_weights = state.current_weights;
            let src = state.current_weights;
            state.mutate(&src, config.mutation_spread);
            state.temperature = config.initial_temperature;
        } else {
            let delta_l = latency - state.prev_latency;
            let prob = (-delta_l / state.temperature as f64).exp();

            if delta_l < 0.0 || rng.gen_bool(prob.clamp(0.0, 1.0)) {
                log::debug!(
                    "Optimizer: Explore - Accepted new weights (latency: {:.2}ns, temp: {:.2})",
                    latency,
                    state.temperature
                );
                state.prev_weights = state.current_weights;
                state.prev_latency = latency;
                let src = state.current_weights;
                state.mutate(&src, config.mutation_spread);
                state.temperature *= config.cooling_factor;
            } else {
                log::debug!(
                    "Optimizer: Exploit - Reverting weights (latency: {:.2}ns, temp: {:.2})",
                    latency,
                    state.temperature
                );
                state.current_weights = state.prev_weights;
                let src = state.prev_weights;
                state.mutate(&src, config.mutation_spread);
                state.temperature *= config.cooling_factor;
            }
        }

        for id in &state.active_devices {
            let i = id.as_u16() as usize;
            shared.0[i].store(state.current_weights[i].to_bits(), Ordering::Release);
        }
    }
}
