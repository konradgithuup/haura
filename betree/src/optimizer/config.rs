use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct OptimizerConfig {
    pub cooling_factor: f32,
    /// Maximum variance for weight mutations.
    pub mutation_spread: f32,
    /// Relative latency change threshold for Hibernation (e.g., 0.01 for 1%).
    pub min_change_ratio: f64,
    /// Relative latency spike threshold for Wake-Up (e.g., 0.5 for 50%).
    pub max_idle_ratio: f64,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            cooling_factor: 0.95,
            mutation_spread: 0.1,
            min_change_ratio: 0.01,
            max_idle_ratio: 0.5,
        }
    }
}
