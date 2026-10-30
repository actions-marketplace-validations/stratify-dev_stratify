use serde::Serialize;
use stratify_core::{Finding, Report, Severity, Span};

/// Render a report as a SARIF 2.1.0 document. GitHub code scanning and GitLab
/// render this as inline annotations.
pub fn render(report: &Report) -> String {
    serde_json::to_string_pretty(&build(report)).expect("sarif serializes")
}

fn level_of(sev: Severity) -> &'static str {
    match sev {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "note",
    }
}

fn sarif_rule(id: &str) -> SarifRule {
    match stratify_core::rule::rule(id) {
        Some(m) => SarifRule {
            id: id.to_string(),
            name: m.name.to_string(),
            short_description: Text {
                text: m.description.to_string(),
            },
            full_description: Some(Text {
                text: m.description.to_string(),
            }),
            help: Some(Text {
                text: m.remediation.to_string(),
            }),
            default_configuration: Some(DefaultConfiguration {
                level: level_of(m.default_severity),
            }),
            properties: Some(RuleProperties {
                category: m.category,
                tags: m.tags.to_vec(),
            }),
        },
        None => SarifRule {
            id: id.to_string(),
            name: id.to_string(),
            short_description: Text {
                text: "Stratify finding.".to_string(),
            },
            full_description: None,
            help: None,
            default_configuration: None,
            properties: None,
        },
    }
}

fn physical(span: &Span) -> PhysicalLocation {
    PhysicalLocation {
        artifact_location: ArtifactLocation {
            uri: span.file.clone(),
        },
        region: Region {
            start_line: span.start_line.max(1),
        },
    }
}

fn result_of(f: &Finding) -> SarifResult {
    let related_locations = f
        .related
        .iter()
        .enumerate()
        .map(|(id, r)| RelatedLocation {
            id,
            physical_location: physical(&r.span),
            message: Text {
                text: r.message.clone(),
            },
        })
        .collect();

    // A reason has no location, so only `path` produces thread-flow locations.
    // SARIF requires non-empty `threadFlows` and `locations`, so no path means
    // no code flow.
    let code_flows = if f.path.is_empty() {
        Vec::new()
    } else {
        vec![CodeFlow {
            message: (!f.reasons.is_empty()).then(|| Text {
                text: f.reasons.join("; "),
            }),
            thread_flows: vec![ThreadFlow {
                locations: f
                    .path
                    .iter()
                    .enumerate()
                    .map(|(k, step)| ThreadFlowLocation {
                        location: FlowLocation {
                            physical_location: physical(&step.span),
                            message: Text {
                                text: step.message.clone(),
                            },
                        },
                        execution_order: k + 1,
                    })
                    .collect(),
            }],
        }]
    };

    SarifResult {
        rule_id: f.rule.clone(),
        level: level_of(f.severity),
        message: Text {
            text: f.message.clone(),
        },
        locations: vec![Location {
            physical_location: physical(&f.span),
        }],
        related_locations,
        code_flows,
        properties: (!f.reasons.is_empty()).then(|| ResultProperties {
            reasons: f.reasons.clone(),
        }),
    }
}

fn build(report: &Report) -> Sarif {
    // Distinct rule ids, in first-seen order, become the driver's rule metadata.
    let mut seen: Vec<String> = Vec::new();
    for f in &report.findings {
        if !seen.contains(&f.rule) {
            seen.push(f.rule.clone());
        }
    }
    let rules = seen.iter().map(|id| sarif_rule(id)).collect();

    let results = report.findings.iter().map(result_of).collect();

    Sarif {
        schema: "https://json.schemastore.org/sarif-2.1.0.json",
        version: "2.1.0",
        runs: vec![Run {
            tool: Tool {
                driver: Driver {
                    name: "Stratify",
                    information_uri: "https://github.com/stratify-dev/stratify",
                    version: env!("CARGO_PKG_VERSION"),
                    rules,
                },
            },
            results,
        }],
    }
}

#[derive(Serialize)]
struct Sarif {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: Vec<Run>,
}

#[derive(Serialize)]
struct Run {
    tool: Tool,
    results: Vec<SarifResult>,
}

#[derive(Serialize)]
struct Tool {
    driver: Driver,
}

#[derive(Serialize)]
struct Driver {
    name: &'static str,
    #[serde(rename = "informationUri")]
    information_uri: &'static str,
    version: &'static str,
    rules: Vec<SarifRule>,
}

#[derive(Serialize)]
struct SarifRule {
    id: String,
    name: String,
    #[serde(rename = "shortDescription")]
    short_description: Text,
    #[serde(rename = "fullDescription", skip_serializing_if = "Option::is_none")]
    full_description: Option<Text>,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<Text>,
    #[serde(
        rename = "defaultConfiguration",
        skip_serializing_if = "Option::is_none"
    )]
    default_configuration: Option<DefaultConfiguration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<RuleProperties>,
}

#[derive(Serialize)]
struct DefaultConfiguration {
    level: &'static str,
}

#[derive(Serialize)]
struct RuleProperties {
    category: &'static str,
    tags: Vec<&'static str>,
}

#[derive(Serialize)]
struct SarifResult {
    #[serde(rename = "ruleId")]
    rule_id: String,
    level: &'static str,
    message: Text,
    locations: Vec<Location>,
    #[serde(rename = "relatedLocations", skip_serializing_if = "Vec::is_empty")]
    related_locations: Vec<RelatedLocation>,
    #[serde(rename = "codeFlows", skip_serializing_if = "Vec::is_empty")]
    code_flows: Vec<CodeFlow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<ResultProperties>,
}

#[derive(Serialize)]
struct RelatedLocation {
    id: usize,
    #[serde(rename = "physicalLocation")]
    physical_location: PhysicalLocation,
    message: Text,
}

#[derive(Serialize)]
struct CodeFlow {
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<Text>,
    #[serde(rename = "threadFlows")]
    thread_flows: Vec<ThreadFlow>,
}

#[derive(Serialize)]
struct ThreadFlow {
    locations: Vec<ThreadFlowLocation>,
}

#[derive(Serialize)]
struct ThreadFlowLocation {
    location: FlowLocation,
    #[serde(rename = "executionOrder")]
    execution_order: usize,
}

#[derive(Serialize)]
struct FlowLocation {
    #[serde(rename = "physicalLocation")]
    physical_location: PhysicalLocation,
    message: Text,
}

#[derive(Serialize)]
struct ResultProperties {
    reasons: Vec<String>,
}

#[derive(Serialize)]
struct Location {
    #[serde(rename = "physicalLocation")]
    physical_location: PhysicalLocation,
}

#[derive(Serialize)]
struct PhysicalLocation {
    #[serde(rename = "artifactLocation")]
    artifact_location: ArtifactLocation,
    region: Region,
}

#[derive(Serialize)]
struct ArtifactLocation {
    uri: String,
}

#[derive(Serialize)]
struct Region {
    #[serde(rename = "startLine")]
    start_line: usize,
}

#[derive(Serialize)]
struct Text {
    text: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use stratify_core::ir::Span;
    use stratify_core::{Confidence, Finding, Severity};

    fn finding(rule: &str, sev: Severity, file: &str, line: usize) -> Finding {
        Finding::new(
            rule,
            sev,
            format!("{rule} message"),
            Span {
                file: file.into(),
                start_byte: 0,
                end_byte: 1,
                start_line: line,
            },
            Confidence::Certain,
        )
    }

    fn span_at(file: &str, line: usize) -> Span {
        Span {
            file: file.into(),
            start_byte: 0,
            end_byte: 1,
            start_line: line,
        }
    }

    #[test]
    fn related_spans_become_related_locations() {
        let f = finding("duplication", Severity::Warning, "a.rb", 1)
            .with_related(span_at("b.rb", 4), "clone", "same block")
            .with_related(span_at("c.rb", 7), "clone", "same block");
        let v: serde_json::Value = serde_json::from_str(&render(&Report::new(vec![f]))).unwrap();
        let rel = v["runs"][0]["results"][0]["relatedLocations"]
            .as_array()
            .unwrap();
        assert_eq!(rel.len(), 2);
        assert_eq!(rel[1]["id"], 1);
        assert_eq!(
            rel[1]["physicalLocation"]["artifactLocation"]["uri"],
            "c.rb"
        );
        assert_eq!(rel[1]["physicalLocation"]["region"]["startLine"], 7);
        assert_eq!(rel[1]["message"]["text"], "same block");
        assert!(v["runs"][0]["results"][0].get("codeFlows").is_none());
    }

    #[test]
    fn path_becomes_one_ordered_thread_flow() {
        let f = finding("cycle", Severity::Warning, "a.rb", 1)
            .with_path_step(span_at("a.rb", 1), "cycle_member", "imports b.rb")
            .with_path_step(span_at("b.rb", 2), "cycle_member", "imports c.rb")
            .with_path_step(span_at("c.rb", 3), "cycle_member", "imports a.rb")
            .with_reason("each file imports the next")
            .with_reason("the last imports the first");
        let v: serde_json::Value = serde_json::from_str(&render(&Report::new(vec![f]))).unwrap();
        let result = &v["runs"][0]["results"][0];
        let flows = result["codeFlows"].as_array().unwrap();
        assert_eq!(flows.len(), 1);
        assert_eq!(
            flows[0]["message"]["text"],
            "each file imports the next; the last imports the first"
        );
        let threads = flows[0]["threadFlows"].as_array().unwrap();
        assert_eq!(threads.len(), 1);
        let locs = threads[0]["locations"].as_array().unwrap();
        let uris: Vec<&str> = locs
            .iter()
            .map(|l| {
                l["location"]["physicalLocation"]["artifactLocation"]["uri"]
                    .as_str()
                    .unwrap()
            })
            .collect();
        assert_eq!(uris, vec!["a.rb", "b.rb", "c.rb"]);
        let orders: Vec<u64> = locs
            .iter()
            .map(|l| l["executionOrder"].as_u64().unwrap())
            .collect();
        assert_eq!(orders, vec![1, 2, 3]);
        assert_eq!(locs[2]["location"]["message"]["text"], "imports a.rb");
        assert_eq!(
            result["properties"]["reasons"][1],
            "the last imports the first"
        );
    }

    #[test]
    fn reasons_without_path_go_to_properties_only() {
        let f = finding("dead_code", Severity::Warning, "a.rb", 1).with_reason("no caller");
        let v: serde_json::Value = serde_json::from_str(&render(&Report::new(vec![f]))).unwrap();
        let result = &v["runs"][0]["results"][0];
        assert_eq!(result["properties"]["reasons"][0], "no caller");
        assert!(result.get("codeFlows").is_none());
    }

    #[test]
    fn result_without_evidence_has_no_evidence_keys() {
        let v: serde_json::Value = serde_json::from_str(&render(&Report::new(vec![finding(
            "cycle",
            Severity::Warning,
            "a.rb",
            1,
        )])))
        .unwrap();
        let r = v["runs"][0]["results"][0].as_object().unwrap();
        assert!(!r.contains_key("relatedLocations"));
        assert!(!r.contains_key("codeFlows"));
        assert!(!r.contains_key("properties"));
    }

    #[test]
    fn renders_valid_sarif_shape() {
        let report = Report::new(vec![
            finding("dead_code", Severity::Warning, "A.java", 5),
            finding("complexity", Severity::Info, "b.rb", 1),
        ]);
        let v: serde_json::Value = serde_json::from_str(&render(&report)).unwrap();
        assert_eq!(v["version"], "2.1.0");
        assert_eq!(v["runs"][0]["tool"]["driver"]["name"], "Stratify");
        // two distinct rules
        assert_eq!(
            v["runs"][0]["tool"]["driver"]["rules"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        // two results
        let results = v["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["ruleId"], "dead_code");
        assert_eq!(results[0]["level"], "warning");
        assert_eq!(
            results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "A.java"
        );
        assert_eq!(
            results[0]["locations"][0]["physicalLocation"]["region"]["startLine"],
            5
        );
        // Info maps to SARIF "note"
        assert_eq!(results[1]["level"], "note");
    }

    #[test]
    fn empty_report_is_valid_sarif() {
        let v: serde_json::Value = serde_json::from_str(&render(&Report::new(vec![]))).unwrap();
        assert_eq!(v["version"], "2.1.0");
        assert!(v["runs"][0]["results"].as_array().unwrap().is_empty());
        assert!(v["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn distinct_rules_dedupe_in_driver() {
        let report = Report::new(vec![
            finding("dead_code", Severity::Warning, "a", 1),
            finding("dead_code", Severity::Warning, "b", 2),
        ]);
        let v: serde_json::Value = serde_json::from_str(&render(&report)).unwrap();
        assert_eq!(
            v["runs"][0]["tool"]["driver"]["rules"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(v["runs"][0]["results"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn driver_rules_carry_registry_metadata() {
        let report = Report::new(vec![finding("cycle", Severity::Warning, "a.rb", 1)]);
        let v: serde_json::Value = serde_json::from_str(&render(&report)).unwrap();
        let r = &v["runs"][0]["tool"]["driver"]["rules"][0];
        assert_eq!(r["id"], "cycle");
        assert_eq!(r["name"], "Dependency cycle");
        assert!(r["help"]["text"]
            .as_str()
            .unwrap()
            .contains("Break the cycle"));
        assert_eq!(r["properties"]["category"], "architecture");
        assert_eq!(r["defaultConfiguration"]["level"], "warning");
    }

    #[test]
    fn unknown_rule_still_renders() {
        let report = Report::new(vec![finding("custom", Severity::Info, "a.rb", 1)]);
        let v: serde_json::Value = serde_json::from_str(&render(&report)).unwrap();
        let r = &v["runs"][0]["tool"]["driver"]["rules"][0];
        assert_eq!(r["id"], "custom");
        assert_eq!(r["shortDescription"]["text"], "Stratify finding.");
    }
}
