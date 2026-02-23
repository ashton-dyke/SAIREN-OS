//! Online K-Means regime clustering of CfC motor neuron outputs.
//!
//! Clusters the 8-dimensional motor output vectors from the CfC NCP network
//! into k=4 drilling regimes using online k-means with a fixed learning rate.
//! Centroids are initialised lazily from the first 4 distinct motor output vectors.

/// Number of regime clusters.
const K: usize = 4;
/// Dimensionality of motor output vectors.
const DIM: usize = 8;
/// Learning rate for online centroid updates.
const LEARNING_RATE: f64 = 0.01;
/// Minimum squared Euclidean distance to consider two points "distinct" during init.
const DISTINCT_THRESHOLD_SQ: f64 = 1e-12;

/// Online k-means clusterer for CfC motor neuron outputs.
#[derive(Debug, Clone)]
pub struct RegimeClusterer {
    centroids: [[f64; DIM]; K],
    init_buffer: Vec<[f64; DIM]>,
    initialized: bool,
    latest_regime_id: u8,
}

impl RegimeClusterer {
    /// Create a new clusterer with zeroed centroids.
    pub fn new() -> Self {
        Self {
            centroids: [[0.0; DIM]; K],
            init_buffer: Vec::with_capacity(K),
            initialized: false,
            latest_regime_id: 0,
        }
    }

    /// Assign a motor output vector to the nearest regime and update the centroid.
    ///
    /// During the initialisation phase (fewer than 4 distinct points seen),
    /// points are buffered and regime 0 is returned. Once 4 distinct points
    /// have been collected, centroids are seeded and online updates begin.
    pub fn assign(&mut self, motor_outputs: &[f64]) -> u8 {
        let mut point = [0.0; DIM];
        let len = motor_outputs.len().min(DIM);
        point[..len].copy_from_slice(&motor_outputs[..len]);

        if !self.initialized {
            self.try_init(point);
            return self.latest_regime_id;
        }

        // Find nearest centroid
        let mut best_k = 0usize;
        let mut best_dist = f64::MAX;
        for (i, centroid) in self.centroids.iter().enumerate() {
            let dist = sq_dist(&point, centroid);
            if dist < best_dist {
                best_dist = dist;
                best_k = i;
            }
        }

        // Nudge centroid towards point
        for d in 0..DIM {
            self.centroids[best_k][d] += LEARNING_RATE * (point[d] - self.centroids[best_k][d]);
        }

        self.latest_regime_id = best_k as u8;
        self.latest_regime_id
    }

    /// Get a copy of the current centroids.
    pub fn centroids(&self) -> [[f64; DIM]; K] {
        self.centroids
    }

    /// Whether the clusterer has been initialised with 4 distinct points.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Reset to uninitialised state.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Try to add a point to the init buffer. If we reach K distinct points, seed centroids.
    fn try_init(&mut self, point: [f64; DIM]) {
        // Check if this point is distinct from all buffered points
        let is_distinct = self.init_buffer.iter().all(|existing| {
            sq_dist(existing, &point) > DISTINCT_THRESHOLD_SQ
        });

        if is_distinct {
            self.init_buffer.push(point);
        }

        if self.init_buffer.len() >= K {
            for (i, seed) in self.init_buffer.iter().take(K).enumerate() {
                self.centroids[i] = *seed;
            }
            self.initialized = true;
            // Assign the current point now that we're initialized
            // (it's one of the seed points, so regime will be its index)
            let mut best_k = 0usize;
            let mut best_dist = f64::MAX;
            for (i, centroid) in self.centroids.iter().enumerate() {
                let dist = sq_dist(&point, centroid);
                if dist < best_dist {
                    best_dist = dist;
                    best_k = i;
                }
            }
            self.latest_regime_id = best_k as u8;
        }
    }
}

/// Squared Euclidean distance between two 8-d points.
fn sq_dist(a: &[f64; DIM], b: &[f64; DIM]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_with_four_distinct_points() {
        let mut rc = RegimeClusterer::new();
        assert!(!rc.is_initialized());

        let points: [[f64; 8]; 4] = [
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        ];

        // First 3 points: still not initialized
        for p in &points[..3] {
            let regime = rc.assign(p);
            assert_eq!(regime, 0);
            assert!(!rc.is_initialized());
        }

        // 4th distinct point: now initialized
        let regime = rc.assign(&points[3]);
        assert!(rc.is_initialized());
        // Should be assigned to centroid 3 (last seed)
        assert_eq!(regime, 3);
    }

    #[test]
    fn test_duplicate_points_dont_count_for_init() {
        let mut rc = RegimeClusterer::new();

        let point = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];

        // Feed the same point 10 times
        for _ in 0..10 {
            rc.assign(&point);
        }
        assert!(!rc.is_initialized());
        assert_eq!(rc.init_buffer.len(), 1);
    }

    #[test]
    fn test_assignment_to_nearest() {
        let mut rc = RegimeClusterer::new();

        // Seed with 4 well-separated points
        let seeds: [[f64; 8]; 4] = [
            [10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 0.0],
        ];
        for s in &seeds {
            rc.assign(s);
        }
        assert!(rc.is_initialized());

        // A point near seed 1 should be assigned to regime 1
        let near_1 = [0.1, 9.9, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0];
        assert_eq!(rc.assign(&near_1), 1);

        // A point near seed 2 should be assigned to regime 2
        let near_2 = [0.0, 0.1, 9.8, 0.0, 0.0, 0.0, 0.0, 0.0];
        assert_eq!(rc.assign(&near_2), 2);
    }

    #[test]
    fn test_centroid_nudge() {
        let mut rc = RegimeClusterer::new();

        let seeds: [[f64; 8]; 4] = [
            [10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 0.0],
        ];
        for s in &seeds {
            rc.assign(s);
        }

        let before = rc.centroids()[0];

        // Push a point near centroid 0
        rc.assign(&[11.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        let after = rc.centroids()[0];

        // Centroid 0 should have moved towards 11.0 (by LR * delta)
        assert!(after[0] > before[0], "centroid should nudge towards point");
        let expected = 10.0 + LEARNING_RATE * (11.0 - 10.0);
        assert!((after[0] - expected).abs() < 1e-10);
    }

    #[test]
    fn test_short_motor_outputs() {
        // If motor_outputs is shorter than 8, remaining dims should be zero
        let mut rc = RegimeClusterer::new();
        let short = [1.0, 2.0, 3.0]; // only 3 elements
        rc.assign(&short);
        // Should not panic; first init_buffer entry should have zeros in dims 3-7
        assert_eq!(rc.init_buffer.len(), 1);
        assert_eq!(rc.init_buffer[0][3], 0.0);
    }

    #[test]
    fn test_reset() {
        let mut rc = RegimeClusterer::new();
        let seeds: [[f64; 8]; 4] = [
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        ];
        for s in &seeds {
            rc.assign(s);
        }
        assert!(rc.is_initialized());

        rc.reset();
        assert!(!rc.is_initialized());
        assert!(rc.init_buffer.is_empty());
    }
}
