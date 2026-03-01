//! Template-based narrative generation for post-well debrief

use crate::types::{AnomalyCategory, FeedbackSummary, FormationComparison, TimelineEvent};

/// Generate a human-readable debrief narrative from structured data.
///
/// Produces sections: Summary, Formation Performance, Advisory Timeline
/// Highlights, Feedback Summary, and Lessons Learned.
pub fn generate_narrative(
    well_id: &str,
    total_depth_ft: f64,
    total_bit_hours: f64,
    comparisons: &[FormationComparison],
    feedback: &FeedbackSummary,
    timeline: &[TimelineEvent],
) -> String {
    let mut sections = Vec::new();

    // 1. Summary
    sections.push(format!(
        "## Summary\n\n\
         Well {} reached {:.0} ft TD in {:.1} bit hours across {} formation(s). \
         A total of {} advisories were generated during the well.",
        well_id,
        total_depth_ft,
        total_bit_hours,
        comparisons.len(),
        timeline.len(),
    ));

    // 2. Formation Performance
    if !comparisons.is_empty() {
        let mut perf_lines = vec!["## Formation Performance\n".to_string()];
        for comp in comparisons {
            let planned_str = match comp.planned_rop_ft_hr {
                Some(p) => format!("{:.1} ft/hr (planned)", p),
                None => "N/A (no plan data)".to_string(),
            };
            let delta_str = match comp.rop_delta_pct {
                Some(d) if d > 0.0 => format!("+{:.1}%", d),
                Some(d) => format!("{:.1}%", d),
                None => "N/A".to_string(),
            };
            let assessment_display = match comp.assessment.as_str() {
                "exceeded_plan" => "EXCEEDED PLAN",
                "met_plan" => "MET PLAN",
                "below_plan" => "BELOW PLAN",
                _ => "NO PLAN DATA",
            };

            perf_lines.push(format!(
                "- **{}** ({:.0}-{:.0} ft): Actual avg ROP {:.1} ft/hr vs {}. \
                 Delta: {}. Assessment: **{}**. \
                 {} advisory(ies), {} critical.",
                comp.formation_name,
                comp.depth_top_ft,
                comp.depth_base_ft,
                comp.actual_avg_rop_ft_hr,
                planned_str,
                delta_str,
                assessment_display,
                comp.advisory_count,
                comp.critical_count,
            ));
        }
        sections.push(perf_lines.join("\n"));
    }

    // 3. Advisory Timeline Highlights
    let critical_events: Vec<&TimelineEvent> = timeline
        .iter()
        .filter(|e| e.severity == "CRITICAL")
        .collect();
    let mechanical_events: Vec<&TimelineEvent> = timeline
        .iter()
        .filter(|e| e.category == AnomalyCategory::Mechanical)
        .collect();

    if !critical_events.is_empty() || !mechanical_events.is_empty() {
        let mut highlight_lines = vec!["## Advisory Timeline Highlights\n".to_string()];

        if !critical_events.is_empty() {
            highlight_lines.push(format!(
                "- **Critical advisories**: {} event(s)",
                critical_events.len()
            ));
            for evt in critical_events.iter().take(5) {
                let fm = evt
                    .formation_name
                    .as_deref()
                    .unwrap_or("unknown formation");
                highlight_lines.push(format!(
                    "  - At {:.0} ft ({}): {}",
                    evt.depth_ft, fm, evt.recommendation
                ));
            }
        }

        if !mechanical_events.is_empty() {
            highlight_lines.push(format!(
                "- **Mechanical events**: {} event(s) (stick-slip, vibration, torque)",
                mechanical_events.len()
            ));
        }

        sections.push(highlight_lines.join("\n"));
    }

    // 4. Feedback Summary
    if feedback.total_feedback > 0 {
        let mut fb_lines = vec!["## Feedback Summary\n".to_string()];
        fb_lines.push(format!(
            "- {} of {} advisories received operator feedback ({:.0}% coverage).",
            feedback.total_feedback,
            feedback.total_advisories,
            if feedback.total_advisories > 0 {
                feedback.total_feedback as f64 / feedback.total_advisories as f64 * 100.0
            } else {
                0.0
            },
        ));
        fb_lines.push(format!(
            "- Overall confirmation rate: **{:.0}%** ({} confirmed, {} false positive, {} unclear).",
            feedback.confirmation_rate * 100.0,
            feedback.confirmed,
            feedback.false_positives,
            feedback.unclear,
        ));

        for cr in &feedback.category_rates {
            if cr.total > 0 {
                fb_lines.push(format!(
                    "  - {:?}: {:.0}% confirmed ({}/{})",
                    cr.category,
                    cr.confirmation_rate * 100.0,
                    cr.confirmed,
                    cr.total,
                ));
            }
        }

        if feedback.confirmation_rate < 0.5 && feedback.total_feedback >= 10 {
            fb_lines.push(
                "- **Suggestion**: Low confirmation rate indicates thresholds may need tightening. \
                 Review /api/v2/config/suggestions for specific recommendations."
                    .to_string(),
            );
        }

        sections.push(fb_lines.join("\n"));
    }

    // 5. Lessons Learned
    let exceeded: Vec<&FormationComparison> = comparisons
        .iter()
        .filter(|c| c.assessment == "exceeded_plan")
        .collect();
    let below: Vec<&FormationComparison> = comparisons
        .iter()
        .filter(|c| c.assessment == "below_plan")
        .collect();

    if !exceeded.is_empty() || !below.is_empty() {
        let mut lessons = vec!["## Lessons Learned\n".to_string()];

        if !exceeded.is_empty() {
            lessons.push("**What worked:**".to_string());
            for comp in &exceeded {
                lessons.push(format!(
                    "- {} exceeded planned ROP by {:.0}% (actual {:.1} vs planned {:.1} ft/hr). \
                     Best params: WOB {:.1} klbs, RPM {:.0}.",
                    comp.formation_name,
                    comp.rop_delta_pct.unwrap_or(0.0),
                    comp.actual_avg_rop_ft_hr,
                    comp.planned_rop_ft_hr.unwrap_or(0.0),
                    comp.actual_best_wob_klbs,
                    comp.actual_best_rpm,
                ));
            }
        }

        if !below.is_empty() {
            lessons.push("\n**Areas for improvement:**".to_string());
            for comp in &below {
                let mech_note = if comp.mechanical_count > 0 {
                    format!(
                        " ({} mechanical event(s) may have contributed)",
                        comp.mechanical_count
                    )
                } else {
                    String::new()
                };
                lessons.push(format!(
                    "- {} fell {:.0}% below planned ROP (actual {:.1} vs planned {:.1} ft/hr).{}",
                    comp.formation_name,
                    comp.rop_delta_pct.unwrap_or(0.0).abs(),
                    comp.actual_avg_rop_ft_hr,
                    comp.planned_rop_ft_hr.unwrap_or(0.0),
                    mech_note,
                ));
            }
        }

        sections.push(lessons.join("\n"));
    }

    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CategoryFeedbackRate, FeedbackSummary, FormationComparison};

    fn make_comparison(name: &str, actual_rop: f64, planned_rop: f64) -> FormationComparison {
        let delta = if planned_rop > 0.0 {
            Some((actual_rop - planned_rop) / planned_rop * 100.0)
        } else {
            None
        };
        let assessment = match delta {
            Some(d) if d > 10.0 => "exceeded_plan",
            Some(d) if d < -10.0 => "below_plan",
            Some(_) => "met_plan",
            None => "no_plan_data",
        };
        FormationComparison {
            formation_name: name.to_string(),
            depth_top_ft: 0.0,
            depth_base_ft: 3000.0,
            planned_wob_klbs: Some(20.0),
            planned_rpm: Some(120.0),
            planned_rop_ft_hr: Some(planned_rop),
            actual_avg_rop_ft_hr: actual_rop,
            actual_best_rop_ft_hr: actual_rop * 1.2,
            actual_avg_mse_psi: 15000.0,
            actual_best_wob_klbs: 25.0,
            actual_best_rpm: 130.0,
            rop_delta_pct: delta,
            advisory_count: 3,
            critical_count: 1,
            mechanical_count: 0,
            assessment: assessment.to_string(),
        }
    }

    #[test]
    fn test_narrative_generation() {
        let comparisons = vec![
            make_comparison("Shallow", 100.0, 80.0),
            make_comparison("Deep", 30.0, 60.0),
        ];
        let feedback = FeedbackSummary {
            total_advisories: 10,
            total_feedback: 8,
            confirmed: 6,
            false_positives: 1,
            unclear: 1,
            confirmation_rate: 0.857,
            category_rates: vec![CategoryFeedbackRate {
                category: AnomalyCategory::DrillingEfficiency,
                total: 5,
                confirmed: 4,
                false_positives: 1,
                confirmation_rate: 0.8,
            }],
        };
        let timeline = vec![TimelineEvent {
            timestamp: 1000,
            depth_ft: 1500.0,
            formation_name: Some("Shallow".to_string()),
            category: AnomalyCategory::DrillingEfficiency,
            severity: "CRITICAL".to_string(),
            recommendation: "Reduce WOB".to_string(),
            feedback: Some("confirmed".to_string()),
        }];

        let narrative = generate_narrative("Well-A", 6000.0, 100.0, &comparisons, &feedback, &timeline);

        assert!(!narrative.is_empty());
        assert!(narrative.contains("## Summary"));
        assert!(narrative.contains("Well Well-A"));
        assert!(narrative.contains("## Formation Performance"));
        assert!(narrative.contains("EXCEEDED PLAN"));
        assert!(narrative.contains("BELOW PLAN"));
        assert!(narrative.contains("## Feedback Summary"));
        assert!(narrative.contains("## Lessons Learned"));
        assert!(narrative.contains("What worked"));
        assert!(narrative.contains("Areas for improvement"));
    }
}
