//! Physics-based fatigue and wear models

/// Miner's Rule for cumulative fatigue damage
///
/// Calculates damage accumulation based on load cycles.
/// D = sum(n_i / N_i) where n = actual cycles, N = cycles to failure at that load
///
/// For MVP: Simple linear accumulation (damage = cycles / rating)
pub fn miners_rule(cycles: u64, rating: f64) -> f64 {
    if rating <= 0.0 {
        return 1.0; // Fully damaged if invalid rating
    }
    cycles as f64 / rating
}

/// ISO 281 bearing life calculation (L10 life in hours)
///
/// L10 = (1,000,000 / (60 * rpm)) * (C / P)^3
///
/// Where:
/// - C = dynamic load rating (rating parameter)
/// - P = equivalent dynamic bearing load (load parameter)
/// - rpm = rotational speed
///
/// Returns estimated hours until 10% of bearings would fail
pub fn l10_life(rpm: f64, load: f64, rating: f64) -> f64 {
    if rpm <= 0.0 || load <= 0.0 || rating <= 0.0 {
        return 0.0;
    }

    let load_ratio = rating / load;
    (1_000_000.0 / (60.0 * rpm)) * load_ratio.powi(3)
}

/// Calculate wear acceleration (2nd derivative of wear history)
///
/// Positive values indicate accelerating wear (concerning)
/// Negative values indicate decelerating wear (stabilizing)
/// Near-zero indicates steady-state wear
pub fn wear_acceleration(history: &[f64]) -> f64 {
    if history.len() < 3 {
        return 0.0;
    }

    // Calculate first derivatives (rate of change)
    let first_derivatives: Vec<f64> = history
        .windows(2)
        .map(|w| w[1] - w[0])
        .collect();

    if first_derivatives.len() < 2 {
        return 0.0;
    }

    // Calculate second derivatives (acceleration)
    let second_derivatives: Vec<f64> = first_derivatives
        .windows(2)
        .map(|w| w[1] - w[0])
        .collect();

    // Return average acceleration
    if second_derivatives.is_empty() {
        return 0.0;
    }

    second_derivatives.iter().sum::<f64>() / second_derivatives.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_miners_rule_basic() {
        // 1000 cycles with rating of 10000 = 10% damage
        let damage = miners_rule(1000, 10000.0);
        assert!((damage - 0.1).abs() < 1e-10);
    }

    #[test]
    fn test_miners_rule_full_damage() {
        // cycles = rating means 100% damage
        let damage = miners_rule(10000, 10000.0);
        assert!((damage - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_l10_life_calculation() {
        // At 250 RPM, with load = rating, L10 should be ~66.67 hours
        // L10 = (1,000,000 / (60 * 250)) * (1)^3 = 66.67
        let life = l10_life(250.0, 100.0, 100.0);
        assert!((life - 66.67).abs() < 0.1, "L10 at load=rating should be ~66.67 hours");
    }

    #[test]
    fn test_l10_life_light_load() {
        // Light load (rating = 2x load) should give 8x life (cubic relationship)
        let life = l10_life(250.0, 50.0, 100.0);
        let expected = 66.67 * 8.0; // ~533 hours
        assert!((life - expected).abs() < 1.0, "Light load should extend life cubically");
    }

    #[test]
    fn test_wear_acceleration_constant() {
        // Constant wear rate should have zero acceleration
        let history = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let accel = wear_acceleration(&history);
        assert!(accel.abs() < 1e-10, "Constant rate should have zero acceleration");
    }

    #[test]
    fn test_wear_acceleration_increasing() {
        // Accelerating wear (exponential-like)
        let history = vec![0.0, 1.0, 3.0, 6.0, 10.0];
        let accel = wear_acceleration(&history);
        assert!(accel > 0.0, "Accelerating wear should be positive");
    }

    #[test]
    fn test_wear_acceleration_decreasing() {
        // Decelerating wear (logarithmic-like)
        let history = vec![0.0, 4.0, 7.0, 9.0, 10.0];
        let accel = wear_acceleration(&history);
        assert!(accel < 0.0, "Decelerating wear should be negative");
    }
}
