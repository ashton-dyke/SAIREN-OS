//! Formation prognosis loader
//!
//! Loads structured pre-drill geological data from TOML files.
//! Search order: `$SAIREN_PROGNOSIS` env var → `./well_prognosis.toml` → None.

use crate::types::{FormationInterval, FormationPrognosis};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

impl FormationPrognosis {
    /// Load prognosis searching:
    /// 1. `$SAIREN_PROGNOSIS` env var
    /// 2. `./well_prognosis.toml` in CWD
    /// 3. Returns None (prognosis is optional)
    pub fn load() -> Option<Self> {
        if let Ok(path) = std::env::var("SAIREN_PROGNOSIS") {
            let p = PathBuf::from(&path);
            if p.exists() {
                return Self::load_from_file(&p);
            }
            warn!(path = %p.display(), "SAIREN_PROGNOSIS file not found");
        }

        let local = PathBuf::from("well_prognosis.toml");
        if local.exists() {
            return Self::load_from_file(&local);
        }

        info!("No formation prognosis found — running without formation context");
        None
    }

    fn load_from_file(path: &Path) -> Option<Self> {
        match std::fs::read_to_string(path) {
            Ok(contents) => match toml::from_str::<Self>(&contents) {
                Ok(prognosis) => {
                    // Validate formations are sorted by depth
                    for w in prognosis.formations.windows(2) {
                        if w[1].depth_top_ft < w[0].depth_top_ft {
                            warn!("Formation intervals not sorted by depth — sorting");
                            let mut sorted = prognosis;
                            sorted.formations.sort_by(|a, b| {
                                a.depth_top_ft
                                    .partial_cmp(&b.depth_top_ft)
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            });
                            info!(
                                path = %path.display(),
                                formations = sorted.formations.len(),
                                "Loaded formation prognosis (re-sorted)"
                            );
                            return Some(sorted);
                        }
                    }
                    info!(
                        path = %path.display(),
                        formations = prognosis.formations.len(),
                        "Loaded formation prognosis"
                    );
                    Some(prognosis)
                }
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "Failed to parse prognosis TOML");
                    None
                }
            },
            Err(e) => {
                warn!(path = %path.display(), error = %e, "Failed to read prognosis file");
                None
            }
        }
    }

    /// Look up the formation at a given bit depth
    pub fn formation_at_depth(&self, depth_ft: f64) -> Option<&FormationInterval> {
        self.formations
            .iter()
            .find(|f| depth_ft >= f.depth_top_ft && depth_ft < f.depth_base_ft)
    }

    /// Look ahead: what formation is next after the current depth
    pub fn next_formation(&self, depth_ft: f64) -> Option<&FormationInterval> {
        self.formations.iter().find(|f| f.depth_top_ft > depth_ft)
    }
}
