//! Formation transition pre-alerting (look-ahead)

use crate::types::{FormationInterval, FormationPrognosis, LookAheadAdvisory};

/// Default look-ahead threshold in minutes
const LOOK_AHEAD_THRESHOLD_MINUTES: f64 = 30.0;

/// Check if the bit is approaching a formation boundary and generate a look-ahead advisory.
///
/// Triggers if the estimated time to the next formation is less than 30 minutes.
pub fn check_look_ahead(
    prognosis: &FormationPrognosis,
    current_depth_ft: f64,
    current_rop_ft_hr: f64,
    current_formation: &FormationInterval,
) -> Option<LookAheadAdvisory> {
    if current_rop_ft_hr <= 0.0 {
        return None;
    }

    let next = prognosis.next_formation(current_depth_ft)?;

    let depth_remaining = next.depth_top_ft - current_depth_ft;
    if depth_remaining <= 0.0 {
        return None;
    }

    let hours_to_next = depth_remaining / current_rop_ft_hr;
    let minutes_to_next = hours_to_next * 60.0;

    if minutes_to_next > LOOK_AHEAD_THRESHOLD_MINUTES {
        return None;
    }

    // Build parameter change recommendations
    let mut parameter_changes = Vec::new();

    let cur = &current_formation.parameters;
    let nxt = &next.parameters;

    let wob_delta = nxt.wob_klbs.optimal - cur.wob_klbs.optimal;
    if wob_delta.abs() > 1.0 {
        let dir = if wob_delta > 0.0 { "increase" } else { "decrease" };
        parameter_changes.push(format!(
            "WOB: {:.0} → {:.0} klbs ({} by {:.0})",
            cur.wob_klbs.optimal,
            nxt.wob_klbs.optimal,
            dir,
            wob_delta.abs()
        ));
    }

    let rpm_delta = nxt.rpm.optimal - cur.rpm.optimal;
    if rpm_delta.abs() > 5.0 {
        let dir = if rpm_delta > 0.0 { "increase" } else { "decrease" };
        parameter_changes.push(format!(
            "RPM: {:.0} → {:.0} ({} by {:.0})",
            cur.rpm.optimal, nxt.rpm.optimal, dir, rpm_delta.abs()
        ));
    }

    let flow_delta = nxt.flow_gpm.optimal - cur.flow_gpm.optimal;
    if flow_delta.abs() > 10.0 {
        let dir = if flow_delta > 0.0 { "increase" } else { "decrease" };
        parameter_changes.push(format!(
            "Flow: {:.0} → {:.0} GPM ({} by {:.0})",
            cur.flow_gpm.optimal,
            nxt.flow_gpm.optimal,
            dir,
            flow_delta.abs()
        ));
    }

    Some(LookAheadAdvisory {
        formation_name: next.name.clone(),
        estimated_minutes: minutes_to_next,
        depth_remaining_ft: depth_remaining,
        parameter_changes,
        hazards: next.hazards.clone(),
        offset_notes: next.offset_performance.notes.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn make_prognosis() -> (FormationPrognosis, FormationInterval) {
        let fm1 = FormationInterval {
            name: "Utsira".to_string(),
            depth_top_ft: 2000.0,
            depth_base_ft: 4000.0,
            lithology: "Sandstone".to_string(),
            hardness: 3.0,
            drillability: "soft".to_string(),
            pore_pressure_ppg: 9.0,
            fracture_gradient_ppg: 13.0,
            hazards: vec![],
            parameters: FormationParameters {
                wob_klbs: ParameterRange { min: 10.0, optimal: 20.0, max: 30.0 },
                rpm: ParameterRange { min: 80.0, optimal: 120.0, max: 160.0 },
                flow_gpm: ParameterRange { min: 400.0, optimal: 500.0, max: 600.0 },
                mud_weight_ppg: 10.0,
                bit_type: "PDC".to_string(),
            },
            offset_performance: OffsetPerformance {
                wells: vec!["W-1".into()],
                avg_rop_ft_hr: 80.0,
                best_rop_ft_hr: 100.0,
                avg_mse_psi: 15000.0,
                best_params: BestParams { wob_klbs: 22.0, rpm: 125.0 },
                notes: String::new(),
            },
        };

        let fm2 = FormationInterval {
            name: "Balder".to_string(),
            depth_top_ft: 4000.0,
            depth_base_ft: 5500.0,
            lithology: "Tuff".to_string(),
            hardness: 7.0,
            drillability: "hard".to_string(),
            pore_pressure_ppg: 11.0,
            fracture_gradient_ppg: 15.0,
            hazards: vec!["Lost circulation risk".into()],
            parameters: FormationParameters {
                wob_klbs: ParameterRange { min: 20.0, optimal: 30.0, max: 40.0 },
                rpm: ParameterRange { min: 60.0, optimal: 90.0, max: 120.0 },
                flow_gpm: ParameterRange { min: 450.0, optimal: 550.0, max: 650.0 },
                mud_weight_ppg: 12.0,
                bit_type: "PDC".to_string(),
            },
            offset_performance: OffsetPerformance {
                wells: vec!["W-1".into(), "W-2".into()],
                avg_rop_ft_hr: 40.0,
                best_rop_ft_hr: 55.0,
                avg_mse_psi: 35000.0,
                best_params: BestParams { wob_klbs: 32.0, rpm: 95.0 },
                notes: "Reduce RPM before entering".into(),
            },
        };

        let prognosis = FormationPrognosis {
            well: PrognosisWellInfo {
                name: "Test-1".into(),
                field: "TestField".into(),
                spud_date: String::new(),
                target_depth_ft: 6000.0,
                coordinate_system: String::new(),
            },
            formations: vec![fm1.clone(), fm2],
            casings: vec![],
        };

        (prognosis, fm1)
    }

    #[test]
    fn triggers_within_30_minutes() {
        let (prognosis, current_fm) = make_prognosis();
        // At 3950 ft, 50 ft from boundary, at 120 ft/hr → 25 min
        let result = check_look_ahead(&prognosis, 3950.0, 120.0, &current_fm);
        assert!(result.is_some(), "Should trigger within 30 min");
        let adv = result.unwrap();
        assert_eq!(adv.formation_name, "Balder");
        assert!(adv.estimated_minutes < 30.0);
        assert!(!adv.hazards.is_empty());
        assert!(adv.offset_notes.contains("Reduce RPM"));
    }

    #[test]
    fn silent_when_far_from_boundary() {
        let (prognosis, current_fm) = make_prognosis();
        // At 2500 ft, 1500 ft from boundary, at 120 ft/hr → 750 min
        let result = check_look_ahead(&prognosis, 2500.0, 120.0, &current_fm);
        assert!(result.is_none(), "Should not trigger far from boundary");
    }

    #[test]
    fn handles_zero_rop() {
        let (prognosis, current_fm) = make_prognosis();
        let result = check_look_ahead(&prognosis, 3950.0, 0.0, &current_fm);
        assert!(result.is_none(), "Should not trigger with zero ROP");
    }

    #[test]
    fn includes_parameter_changes() {
        let (prognosis, current_fm) = make_prognosis();
        let result = check_look_ahead(&prognosis, 3990.0, 200.0, &current_fm);
        let adv = result.unwrap();
        // WOB optimal changes from 20 to 30 → should appear
        assert!(
            adv.parameter_changes.iter().any(|c| c.contains("WOB")),
            "Should recommend WOB change: {:?}",
            adv.parameter_changes
        );
        // RPM optimal changes from 120 to 90 → should appear
        assert!(
            adv.parameter_changes.iter().any(|c| c.contains("RPM")),
            "Should recommend RPM change: {:?}",
            adv.parameter_changes
        );
    }
}
