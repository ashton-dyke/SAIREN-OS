//! Per-formation damping recipe persistence
//!
//! Stores successful damping actions in a sled tree ("damping_recipes"),
//! keyed by formation name. Each formation stores up to
//! `max_recipes_per_formation` recipes (oldest pruned on overflow).
//!
//! Call `init()` after `storage::history::init()`.

use super::history::{get_db, StorageError};
use crate::types::DampingRecipe;
use sled::Tree;
use std::sync::OnceLock;

static RECIPE_TREE: OnceLock<Tree> = OnceLock::new();

/// Initialise the damping recipes sled tree.
///
/// Must be called after `storage::history::init()`.
pub fn init() -> Result<(), StorageError> {
    if RECIPE_TREE.get().is_some() {
        return Ok(());
    }
    let db = get_db()?;
    let tree = db
        .open_tree("damping_recipes")
        .map_err(|e: sled::Error| StorageError::DatabaseError(e.to_string()))?;
    let _ = RECIPE_TREE.set(tree);
    Ok(())
}

fn get_tree() -> Result<&'static Tree, StorageError> {
    RECIPE_TREE.get().ok_or(StorageError::NotInitialized)
}

/// Store a successful damping recipe for the given formation.
/// Appends to the formation's recipe list; prunes oldest if over max_count.
pub fn persist(recipe: &DampingRecipe, max_count: usize) -> Result<(), StorageError> {
    let tree = get_tree()?;
    let key = recipe.formation_name.as_bytes();

    // Read existing recipes for this formation
    let mut recipes: Vec<DampingRecipe> = match tree.get(key)? {
        Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        None => Vec::new(),
    };

    // Append new recipe and prune oldest if over limit
    recipes.push(recipe.clone());
    while recipes.len() > max_count {
        recipes.remove(0);
    }

    let bytes = serde_json::to_vec(&recipes)
        .map_err(|e| StorageError::SerializationError(e.to_string()))?;
    tree.insert(key, bytes)?;
    Ok(())
}

/// Get all recipes for a specific formation.
pub fn get_by_formation(formation_name: &str) -> Vec<DampingRecipe> {
    let tree = match get_tree() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    match tree.get(formation_name.as_bytes()) {
        Ok(Some(bytes)) => serde_json::from_slice(&bytes).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Get the best recipe for a formation (lowest achieved CV).
pub fn best_recipe(formation_name: &str) -> Option<DampingRecipe> {
    let recipes = get_by_formation(formation_name);
    recipes
        .into_iter()
        .min_by(|a, b| a.achieved_cv.partial_cmp(&b.achieved_cv).unwrap_or(std::cmp::Ordering::Equal))
}

/// List all formations that have stored recipes.
pub fn list_formations() -> Vec<String> {
    let tree = match get_tree() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    tree.iter()
        .filter_map(|item| {
            item.ok().and_then(|(k, _)| String::from_utf8(k.to_vec()).ok())
        })
        .collect()
}

/// Load all recipes across all formations.
pub fn load_all() -> Vec<DampingRecipe> {
    let tree = match get_tree() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    tree.iter()
        .filter_map(|item| {
            item.ok().and_then(|(_, v)| {
                serde_json::from_slice::<Vec<DampingRecipe>>(&v).ok()
            })
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_recipe(formation: &str, achieved_cv: f64) -> DampingRecipe {
        DampingRecipe {
            formation_name: formation.to_string(),
            wob_change_pct: -15.0,
            rpm_change_pct: 10.0,
            baseline_cv: 0.25,
            achieved_cv,
            cv_reduction_pct: (0.25 - achieved_cv) / 0.25 * 100.0,
            depth_ft: 10000.0,
            recorded_at: 1000,
        }
    }

    #[test]
    fn test_damping_recipe_serde_roundtrip() {
        let recipe = make_recipe("Sandstone A", 0.12);
        let json = serde_json::to_vec(&recipe).unwrap();
        let decoded: DampingRecipe = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded.formation_name, "Sandstone A");
        assert!((decoded.achieved_cv - 0.12).abs() < 1e-9);
        assert!((decoded.wob_change_pct - (-15.0)).abs() < 1e-9);
    }

    /// Helper: initialise a temporary sled DB for testing.
    ///
    /// Each test gets its own temp dir so they don't interfere.
    fn init_test_db() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);

        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("sairen_recipe_test_{}", id));
        let _ = std::fs::remove_dir_all(&path);

        // Initialise the main sled DB via history module first
        // (damping_recipes depends on the shared sled::Db from history).
        // Since we can't easily point the global OnceLock at a temp dir,
        // use the production init — it's idempotent and safe in tests.
        let _ = crate::storage::history::init(path.to_str().unwrap());
        let _ = super::init();
    }

    #[test]
    fn test_recipe_persist_and_retrieve() {
        init_test_db();

        let recipe = make_recipe("Shale B", 0.10);
        persist(&recipe, 20).expect("persist should succeed");

        let recipes = get_by_formation("Shale B");
        assert_eq!(recipes.len(), 1);
        assert_eq!(recipes[0].formation_name, "Shale B");
        assert!((recipes[0].achieved_cv - 0.10).abs() < 1e-9);
    }

    #[test]
    fn test_recipe_pruning() {
        init_test_db();

        let max_count = 3;
        // Store 5 recipes — only the last 3 should survive
        for i in 0..5 {
            let mut recipe = make_recipe("Limestone C", 0.10 + i as f64 * 0.01);
            recipe.recorded_at = 1000 + i as u64;
            persist(&recipe, max_count).expect("persist should succeed");
        }

        let recipes = get_by_formation("Limestone C");
        assert_eq!(recipes.len(), max_count, "Should prune to max_count");
        // Oldest (recorded_at 1000, 1001) should have been pruned;
        // remaining should be recorded_at 1002, 1003, 1004
        assert_eq!(recipes[0].recorded_at, 1002);
        assert_eq!(recipes[2].recorded_at, 1004);
    }

    #[test]
    fn test_best_recipe_selection() {
        init_test_db();

        // Store 3 recipes with different achieved CVs
        let r1 = make_recipe("Sandstone D", 0.15);
        let r2 = make_recipe("Sandstone D", 0.08); // best (lowest)
        let r3 = make_recipe("Sandstone D", 0.12);

        persist(&r1, 20).unwrap();
        persist(&r2, 20).unwrap();
        persist(&r3, 20).unwrap();

        let best = best_recipe("Sandstone D").expect("Should find a best recipe");
        assert!(
            (best.achieved_cv - 0.08).abs() < 1e-9,
            "Best recipe should have lowest achieved_cv (0.08), got {}",
            best.achieved_cv
        );
    }
}
