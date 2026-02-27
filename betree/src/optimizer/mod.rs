pub mod config;

use crate::data_management::Dml;
use crate::database::RootDmu;
use crate::storage_pool::StoragePoolLayer;
use crate::storage_pool::{DiskOffset, GlobalDiskId};
use rand::Rng;
use seqlock::SeqLock;
use std::sync::Arc;
use std::thread;

/// Maximum number of devices supported by the 12-bit GlobalDiskId addressing scheme.
// TODO: reduce number for this component because we usually don't need that many
pub const MAX_DEVICES: usize = 4096;

/// Shared weight buffer using a Sequence Lock for wait-free reads on the cache path.
pub struct SharedWeights(pub Arc<SeqLock<[f32; MAX_DEVICES]>>);

impl SharedWeights {
    pub fn new() -> Self {
        Self(Arc::new(SeqLock::new([1.0; MAX_DEVICES])))
    }

    /// Fetches a weight for a specific device. Used by the Cache Policy.
    pub fn get_weight(&self, id: GlobalDiskId) -> f32 {
        let weights = self.0.read();
        weights[id.as_u16() as usize]
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
    prev_stats_nanos: [u64; MAX_DEVICES], // (total_nanos, op_count)
    prev_stats_count: [u64; MAX_DEVICES], // (total_nanos, op_count)
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
        }
    }

    fn calculate_epoch_latency(&mut self, dmu: &Arc<RootDmu>) -> f64 {
        let metrics = dmu.spl().metrics();
        let mut total_nanos = 0;
        let mut total_count = 0;

        for (tier_idx, tier) in metrics.tiers.iter().enumerate() {
            if let Some(tier_metrics) = tier {
                for (vdev_idx, stats) in tier_metrics.vdevs.iter().enumerate() {
                    let g_id = DiskOffset::construct_disk_id(tier_idx as u8, vdev_idx as u16);
                    let idx = g_id.as_u16() as usize;

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
        for i in 0..MAX_DEVICES {
            let delta = rng.gen_range(-spread..spread);
            self.current_weights[i] = (source[i] + delta).clamp(0.1, 10.0);
        }
    }
}

pub fn run_optimizer(dmu: Arc<RootDmu>, shared: SharedWeights, config: config::OptimizerConfig) {
    let mut state = OptimizerState::new(&config);
    let mut rng = rand::thread_rng();

    loop {
        thread::sleep(config.epoch_duration);

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
            // Hibernate
            state.current_weights = state.best_weights;
            state.prev_latency = state.best_latency;
        } else if abs_change > config.max_idle && state.temperature < config.min_temperature {
            // Wake-Up
            state.prev_weights = state.current_weights;
            let src = state.current_weights;
            state.mutate(&src, config.mutation_spread);
            state.temperature = config.initial_temperature;
        } else {
            let delta_l = latency - state.prev_latency;
            let prob = (-delta_l / state.temperature as f64).exp();

            if delta_l < 0.0 || rng.gen_bool(prob.clamp(0.0, 1.0)) {
                // Explore
                state.prev_weights = state.current_weights;
                state.prev_latency = latency;
                let src = state.current_weights;
                state.mutate(&src, config.mutation_spread);
                state.temperature *= config.cooling_factor;
            } else {
                // Exploit
                state.current_weights = state.prev_weights;
                let src = state.prev_weights;
                state.mutate(&src, config.mutation_spread);
                state.temperature *= config.cooling_factor;
            }
        }

        shared
            .0
            .lock_write()
            .copy_from_slice(&state.current_weights);
    }
}
