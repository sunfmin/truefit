//! `truefit.toml`: what a repo declares to truefit, read and checked whole.
//! Each part's own checks live with the part; `load` answers every fault at once.

use crate::contract::{Settings, Standin};
use crate::fault;
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

pub const CONFIG: &str = "truefit.toml";

/// All that `truefit.toml` declares.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub contract: Settings,
    #[serde(default)]
    pub standin: Vec<Standin>,
    #[serde(default)]
    pub sim: Option<crate::sim::Sim>,
    #[serde(default)]
    pub model: Vec<crate::model::Model>,
    #[serde(default)]
    pub promises: crate::promise::Sources,
    #[serde(default)]
    pub promise: Vec<crate::promise::Promise>,
}

/// Reads `root/truefit.toml` and checks every part of it: the config, or each fault in it.
pub fn load(root: &Path) -> Result<Config, Vec<Value>> {
    let text = std::fs::read_to_string(root.join(CONFIG)).map_err(|e| {
        vec![fault(
            format!("cannot read {CONFIG} in {}: {e}", root.display()),
            format!("run truefit where {CONFIG} is, or write one (`truefit --help`)"),
        )]
    })?;
    let config: Config = toml::from_str(&text).map_err(|e| {
        vec![fault(
            format!("{CONFIG}: {}", e.message()),
            format!("correct {CONFIG} as `truefit --help` declares it"),
        )]
    })?;
    let mut faults = crate::model::faults(&config.model);
    faults.extend(crate::promise::faults(&config));
    if config.sim.as_ref().is_some_and(|s| s.run.is_empty()) {
        faults.push(fault(
            "[sim] run is empty",
            "declare the command that runs the simulation from a seed, e.g. run = [\"tests/sim\"]",
        ));
    }
    faults.extend(crate::contract::faults(&config.standin));
    if faults.is_empty() {
        Ok(config)
    } else {
        Err(faults)
    }
}
