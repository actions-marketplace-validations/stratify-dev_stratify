use crate::finding::Severity;

/// Static description of one rule. The registry is the single source for
/// rule names, explanations, and fix guidance across every output surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleMeta {
    pub id: &'static str,
    pub name: &'static str,
    /// "maintainability" or "architecture" today. "correctness" and
    /// "security" arrive with flow analysis.
    pub category: &'static str,
    /// Severity of a full-confidence finding. Lower-confidence findings of
    /// the same rule may report lower.
    pub default_severity: Severity,
    pub languages: &'static [&'static str],
    pub tags: &'static [&'static str],
    pub description: &'static str,
    pub remediation: &'static str,
}

pub const ALL_LANGUAGES: &[&str] = &["java", "ruby", "typescript", "python", "go", "rust"];

pub static RULES: &[RuleMeta] = &[
    RuleMeta {
        id: "dead_code",
        name: "Unused function",
        category: "maintainability",
        default_severity: Severity::Warning,
        languages: ALL_LANGUAGES,
        tags: &["dead-code"],
        description: "Unused code: functions never reached from an entrypoint.",
        remediation: "Delete the function, or call it from code that runs. If a framework, reflection, or an external caller reaches it, suppress the finding with a comment naming the reason.",
    },
    RuleMeta {
        id: "duplication",
        name: "Duplicated code",
        category: "maintainability",
        default_severity: Severity::Warning,
        languages: ALL_LANGUAGES,
        tags: &["duplication"],
        description: "Duplicated code blocks.",
        remediation: "Extract the shared block into one function and call it from each site.",
    },
    RuleMeta {
        id: "complexity",
        name: "High cyclomatic complexity",
        category: "maintainability",
        default_severity: Severity::Warning,
        languages: ALL_LANGUAGES,
        tags: &["complexity"],
        description: "Functions with high cyclomatic complexity.",
        remediation: "Split the function. Move each branch body into a named helper, or replace condition chains with a lookup table.",
    },
    RuleMeta {
        id: "hotspot",
        name: "Churn hotspot",
        category: "maintainability",
        default_severity: Severity::Info,
        languages: ALL_LANGUAGES,
        tags: &["complexity", "churn"],
        description: "Complex code that also changes frequently.",
        remediation: "Prioritize this function for tests and refactoring. Frequent edits to complex code are where regressions cluster.",
    },
    RuleMeta {
        id: "cycle",
        name: "Dependency cycle",
        category: "architecture",
        default_severity: Severity::Warning,
        languages: ALL_LANGUAGES,
        tags: &["architecture", "imports"],
        description: "Circular dependencies between files.",
        remediation: "Break the cycle: move the shared code into a new module both sides import, or invert one import through an interface.",
    },
    RuleMeta {
        id: "boundary",
        name: "Layer boundary violation",
        category: "architecture",
        default_severity: Severity::Warning,
        languages: ALL_LANGUAGES,
        tags: &["architecture", "layers"],
        description: "Imports that violate configured layer boundaries.",
        remediation: "Move the import's target into a layer the source may depend on, or route the call through an allowed layer.",
    },
];

/// Look up a rule by id.
pub fn rule(id: &str) -> Option<&'static RuleMeta> {
    RULES.iter().find(|r| r.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emitted_rule_has_metadata() {
        for id in [
            "dead_code",
            "duplication",
            "complexity",
            "hotspot",
            "cycle",
            "boundary",
        ] {
            let r = rule(id).unwrap_or_else(|| panic!("missing rule {id}"));
            assert_eq!(r.id, id);
            assert!(!r.description.is_empty());
            assert!(!r.remediation.is_empty());
            assert_eq!(r.languages.len(), 6, "{id} covers all six languages");
        }
    }

    #[test]
    fn rule_ids_are_unique() {
        let mut ids: Vec<&str> = RULES.iter().map(|r| r.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), RULES.len());
    }

    #[test]
    fn unknown_rule_is_none() {
        assert!(rule("nope").is_none());
    }
}
