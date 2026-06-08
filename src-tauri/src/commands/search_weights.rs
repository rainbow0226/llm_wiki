// DEVWIKI (P4③): SDLC-phase-aware page-type weighting.
//
// The `phase` a query is asked in (design / dev / test / ops) shifts which page
// *types* are most useful: during development you want playbooks and solutions
// first; while designing you want decisions and comparisons. `type_weight`
// returns a multiplier applied to a page's keyword score so those types float
// up for the active phase.
//
// Unknown phase or type → 1.0 (no change), and `phase == None` short-circuits
// to 1.0, so omitting the phase preserves the prior ranking exactly.

/// Multiplier for `page_type` under `phase`. 1.0 means "no adjustment".
pub fn type_weight(phase: Option<&str>, page_type: &str) -> f64 {
    let Some(phase) = phase else {
        return 1.0;
    };
    let phase = phase.trim().to_lowercase();
    match phase.as_str() {
        "design" | "architecture" => match page_type {
            "decision" => 1.5,
            "comparison" => 1.4,
            "concept" => 1.3,
            "synthesis" => 1.2,
            "playbook" => 0.9,
            _ => 1.0,
        },
        "dev" | "develop" | "development" | "build" | "implementation" => match page_type {
            "playbook" => 1.5,
            "solution" => 1.4,
            "entity" => 1.2,
            "decision" => 0.9,
            _ => 1.0,
        },
        "test" | "testing" | "qa" => match page_type {
            "playbook" => 1.5,
            "solution" => 1.3,
            "decision" => 0.9,
            _ => 1.0,
        },
        "ops" | "operations" | "deploy" | "deployment" | "sre" => match page_type {
            "playbook" => 1.5,
            "decision" => 1.2,
            "solution" => 1.2,
            "concept" => 0.9,
            _ => 1.0,
        },
        // Unrecognized phase: do not bias the ranking.
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_phase_or_unknown_phase_is_neutral() {
        assert_eq!(type_weight(None, "playbook"), 1.0);
        assert_eq!(type_weight(Some("sprint-7"), "playbook"), 1.0);
        assert_eq!(type_weight(Some(""), "decision"), 1.0);
    }

    #[test]
    fn dev_phase_favors_playbook_over_decision() {
        assert!(type_weight(Some("dev"), "playbook") > type_weight(Some("dev"), "decision"));
    }

    #[test]
    fn design_phase_favors_decision_over_playbook() {
        assert!(type_weight(Some("design"), "decision") > type_weight(Some("design"), "playbook"));
    }

    #[test]
    fn phase_matching_is_case_insensitive_and_trimmed() {
        assert_eq!(type_weight(Some("  DEV "), "playbook"), type_weight(Some("dev"), "playbook"));
    }

    #[test]
    fn unknown_type_is_neutral_in_known_phase() {
        assert_eq!(type_weight(Some("dev"), "concept"), 1.0);
        assert_eq!(type_weight(Some("ops"), "totally-custom-type"), 1.0);
    }
}
