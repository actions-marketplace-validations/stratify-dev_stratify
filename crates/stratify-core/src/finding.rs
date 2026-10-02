use crate::confidence::Confidence;
use crate::ir::Span;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A secondary location backing a finding: another clone site, a step in a
/// cycle, the file a forbidden import points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Related {
    pub span: Span,
    /// Machine-readable role, e.g. "clone", "cycle_member", "imported_file".
    pub role: String,
    pub message: String,
}

/// One reported problem. `rule` identifies the analysis, e.g. "dead_code".
/// `related`, `path`, and `reasons` are optional evidence, omitted from JSON
/// when empty, so a finding without evidence serializes exactly as in v0.6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub confidence: Confidence,
    /// Supporting locations. No order implied.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<Related>,
    /// Ordered steps of a flow: the files of a cycle, later the hops of a
    /// taint path. SARIF renders it as one thread flow.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<Related>,
    /// Ordered reason chain: why the analysis flagged this.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

impl Finding {
    pub fn new(
        rule: impl Into<String>,
        severity: Severity,
        message: impl Into<String>,
        span: Span,
        confidence: Confidence,
    ) -> Self {
        Finding {
            rule: rule.into(),
            severity,
            message: message.into(),
            span,
            confidence,
            related: Vec::new(),
            path: Vec::new(),
            reasons: Vec::new(),
        }
    }

    pub fn with_related(
        mut self,
        span: Span,
        role: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.related.push(Related {
            span,
            role: role.into(),
            message: message.into(),
        });
        self
    }

    pub fn with_path_step(
        mut self,
        span: Span,
        role: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.path.push(Related {
            span,
            role: role.into(),
            message: message.into(),
        });
        self
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reasons.push(reason.into());
        self
    }
}

/// The top-level machine output. Versioned so downstream consumers can rely on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub findings: Vec<Finding>,
}

impl Report {
    /// Additive optional fields do not bump this. stratify-judge skips judgment for any version it does not know.
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn new(findings: Vec<Finding>) -> Self {
        Report {
            schema_version: Self::SCHEMA_VERSION,
            findings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Span;

    #[test]
    fn severity_ordering_info_lt_warning_lt_error() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn report_serializes_with_schema_version() {
        let report = Report::new(vec![Finding::new(
            "dead_code",
            Severity::Warning,
            "unused method bar",
            Span {
                file: "Foo.java".into(),
                start_byte: 0,
                end_byte: 1,
                start_line: 3,
            },
            Confidence::Certain,
        )]);
        let v: serde_json::Value = serde_json::to_value(&report).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["findings"][0]["rule"], "dead_code");
    }

    fn sp(file: &str, line: usize) -> Span {
        Span {
            file: file.into(),
            start_byte: 0,
            end_byte: 1,
            start_line: line,
        }
    }

    #[test]
    fn finding_without_evidence_omits_new_keys() {
        let f = Finding::new(
            "dead_code",
            Severity::Warning,
            "m",
            sp("a.rs", 1),
            Confidence::Certain,
        );
        let v = serde_json::to_value(&f).unwrap();
        let mut keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            vec!["confidence", "message", "rule", "severity", "span"]
        );
    }

    #[test]
    fn evidence_round_trips_and_keeps_order() {
        let f = Finding::new(
            "cycle",
            Severity::Warning,
            "m",
            sp("a.rs", 1),
            Confidence::Certain,
        )
        .with_path_step(sp("a.rs", 1), "cycle_member", "imports b.rs")
        .with_path_step(sp("b.rs", 3), "cycle_member", "imports a.rs")
        .with_related(sp("c.rs", 9), "clone", "same block")
        .with_reason("first")
        .with_reason("second");
        let json = serde_json::to_string(&f).unwrap();
        let back: Finding = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
        assert_eq!(back.path[1].span.file, "b.rs");
        assert_eq!(back.reasons, vec!["first", "second"]);
    }

    #[test]
    fn v1_report_without_evidence_still_parses() {
        let json = r#"{"schema_version":1,"findings":[{"rule":"dead_code","severity":"warning","message":"m","span":{"file":"a","start_byte":0,"end_byte":1,"start_line":1},"confidence":"certain"}]}"#;
        let r: Report = serde_json::from_str(json).unwrap();
        assert!(r.findings[0].related.is_empty());
        assert!(r.findings[0].path.is_empty());
        assert!(r.findings[0].reasons.is_empty());
    }
}
