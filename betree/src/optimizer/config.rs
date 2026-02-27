use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct OptimizerConfig {
    /// Fixed duration for the optimizer epoch.
    // TODO: Maybe couple this with the WHATT policy eviction epoch
    pub epoch_duration: Duration,
    pub initial_temperature: f32,
    pub min_temperature: f32,
    pub cooling_factor: f32,
    /// Maximum variance for weight mutations.
    pub mutation_spread: f32,
    /// Absolute latency change threshold for Hibernation.
    pub min_change: f64,
    /// Latency spike threshold for Wake-Up.
    pub max_idle: f64,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            epoch_duration: Duration::from_millis(500),
            initial_temperature: 100.0,
            min_temperature: 1.0,
            cooling_factor: 0.95,
            mutation_spread: 0.1,
            min_change: 0.01,
            max_idle: 0.5,
        }
    }
}
