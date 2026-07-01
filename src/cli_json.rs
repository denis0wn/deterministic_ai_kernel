use crate::kernel_types::ReplayCapsule;
use serde_json::Value;

pub fn cli_json_schema_version() -> &'static str {
    "cli-json-v1"
}

pub fn print_json_report(report: &Value) {
    println!("{}", serde_json::to_string_pretty(report).unwrap());
}

pub fn emit_json(command: &str, report: Value) {
    let envelope = command_report(command, report);
    print_json_report(&envelope);
}

pub fn command_report(command: &str, report: Value) -> Value {
    serde_json::json!({
        "ok": true,
        "schema_version": cli_json_schema_version(),
        "command": command,
        "report": report,
    })
}

#[allow(dead_code)]
pub fn capsule_summary_report(
    command: &str,
    task_id: &str,
    capsule: &ReplayCapsule,
    valid: bool,
) -> Value {
    serde_json::json!({
        "ok": true,
        "schema_version": cli_json_schema_version(),
        "command": command,
        "task_id": task_id,
        "execution_id": capsule.execution_id,
        "capsule_id": capsule.capsule_id,
        "valid": valid,
        "events": capsule.event_ids.len(),
        "nodes": capsule.state_graph.nodes.len(),
        "edges": capsule.state_graph.edges.len(),
    })
}

pub struct ComparisonReportInput<'a> {
    pub left_task_id: &'a str,
    pub right_task_id: &'a str,
    pub left_capsule: &'a ReplayCapsule,
    pub right_capsule: &'a ReplayCapsule,
    pub left_valid: bool,
    pub right_valid: bool,
    pub status: &'a str,
    pub explanation: &'a str,
    pub left_only_events: Vec<String>,
    pub right_only_events: Vec<String>,
    pub left_only_nodes: Vec<Value>,
    pub right_only_nodes: Vec<Value>,
    pub left_only_edges: Vec<Value>,
    pub right_only_edges: Vec<Value>,
}

pub fn comparison_report(input: ComparisonReportInput<'_>) -> Value {
    serde_json::json!({
        "ok": true,
        "schema_version": cli_json_schema_version(),
        "left": {
            "task_id": input.left_task_id,
            "capsule_id": input.left_capsule.capsule_id,
            "valid": input.left_valid,
            "events": input.left_capsule.event_ids.len(),
            "nodes": input.left_capsule.state_graph.nodes.len(),
            "edges": input.left_capsule.state_graph.edges.len(),
        },
        "right": {
            "task_id": input.right_task_id,
            "capsule_id": input.right_capsule.capsule_id,
            "valid": input.right_valid,
            "events": input.right_capsule.event_ids.len(),
            "nodes": input.right_capsule.state_graph.nodes.len(),
            "edges": input.right_capsule.state_graph.edges.len(),
        },
        "status": input.status,
        "explanation": input.explanation,
        "diff": {
            "event_ids": {
                "left_only": input.left_only_events,
                "right_only": input.right_only_events,
            },
            "nodes": {
                "left_only": input.left_only_nodes,
                "right_only": input.right_only_nodes,
            },
            "edges": {
                "left_only": input.left_only_edges,
                "right_only": input.right_only_edges,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel_types::{
        ReplayCapsule, StateGraph, StateGraphEdge, StateGraphNode, TrustContext, TrustLevel,
    };
    use serde_json::json;
    use std::collections::BTreeMap;

    fn trust_context() -> TrustContext {
        TrustContext {
            source: "unit-test".into(),
            trust_level: TrustLevel::High,
            verification_status: "verified".into(),
            policy_version: "test-policy-v1".into(),
        }
    }

    fn replay_capsule(
        capsule_id: &str,
        execution_id: &str,
        event_ids: Vec<&str>,
        node_ids: Vec<&str>,
        edge_pairs: Vec<(&str, &str, &str)>,
    ) -> ReplayCapsule {
        ReplayCapsule {
            capsule_id: capsule_id.into(),
            execution_id: execution_id.into(),
            created_at: "2026-06-10T22:20:00Z".into(),
            state_graph: StateGraph {
                nodes: node_ids
                    .into_iter()
                    .map(|id| StateGraphNode {
                        id: id.into(),
                        kind: "event".into(),
                        ref_id: id.into(),
                    })
                    .collect(),
                edges: edge_pairs
                    .into_iter()
                    .map(|(from, to, relation)| StateGraphEdge {
                        from: from.into(),
                        to: to.into(),
                        relation: relation.into(),
                    })
                    .collect(),
            },
            event_ids: event_ids.into_iter().map(|s| s.into()).collect(),
            artifacts: vec![],
            environment: BTreeMap::new(),
            decision_points: vec![],
            determinism_envelope: json!({"mode":"strict"}),
            trust_context: trust_context(),
        }
    }

    #[test]
    fn capsule_summary_report_has_expected_shape() {
        let capsule = replay_capsule(
            "capsule-task-a",
            "task-a",
            vec!["evt-1", "evt-2"],
            vec!["node-1", "node-2"],
            vec![("node-1", "node-2", "causes")],
        );

        let report = capsule_summary_report("replay-capsule", "task-a", &capsule, true);

        assert_eq!(report["ok"], true);
        assert_eq!(report["schema_version"], "cli-json-v1");
        assert_eq!(report["command"], "replay-capsule");
        assert_eq!(report["task_id"], "task-a");
        assert_eq!(report["execution_id"], "task-a");
        assert_eq!(report["capsule_id"], "capsule-task-a");
        assert_eq!(report["valid"], true);
        assert_eq!(report["events"], 2);
        assert_eq!(report["nodes"], 2);
        assert_eq!(report["edges"], 1);
    }

    #[test]
    fn comparison_report_has_expected_shape_and_diff_payloads() {
        let left_capsule = replay_capsule(
            "capsule-left",
            "task-left",
            vec!["evt-1", "evt-2"],
            vec!["node-1", "node-2"],
            vec![("node-1", "node-2", "causes")],
        );

        let right_capsule = replay_capsule(
            "capsule-right",
            "task-right",
            vec!["evt-2", "evt-3"],
            vec!["node-2", "node-3"],
            vec![("node-2", "node-3", "causes")],
        );

        let report = comparison_report(ComparisonReportInput {
            left_task_id: "task-left",
            right_task_id: "task-right",
            left_capsule: &left_capsule,
            right_capsule: &right_capsule,
            left_valid: true,
            right_valid: false,
            status: "divergent",
            explanation: "event ids and graph differ",
            left_only_events: vec!["evt-1".into()],
            right_only_events: vec!["evt-3".into()],
            left_only_nodes: vec![json!({"id":"node-1","kind":"event","ref_id":"node-1"})],
            right_only_nodes: vec![json!({"id":"node-3","kind":"event","ref_id":"node-3"})],
            left_only_edges: vec![json!({"from":"node-1","to":"node-2","relation":"causes"})],
            right_only_edges: vec![json!({"from":"node-2","to":"node-3","relation":"causes"})],
        });

        assert_eq!(report["ok"], true);
        assert_eq!(report["schema_version"], "cli-json-v1");

        assert_eq!(report["left"]["task_id"], "task-left");
        assert_eq!(report["left"]["capsule_id"], "capsule-left");
        assert_eq!(report["left"]["valid"], true);
        assert_eq!(report["left"]["events"], 2);
        assert_eq!(report["left"]["nodes"], 2);
        assert_eq!(report["left"]["edges"], 1);

        assert_eq!(report["right"]["task_id"], "task-right");
        assert_eq!(report["right"]["capsule_id"], "capsule-right");
        assert_eq!(report["right"]["valid"], false);
        assert_eq!(report["right"]["events"], 2);
        assert_eq!(report["right"]["nodes"], 2);
        assert_eq!(report["right"]["edges"], 1);

        assert_eq!(report["status"], "divergent");
        assert_eq!(report["explanation"], "event ids and graph differ");
        assert_eq!(report["diff"]["event_ids"]["left_only"], json!(["evt-1"]));
        assert_eq!(report["diff"]["event_ids"]["right_only"], json!(["evt-3"]));
        assert_eq!(
            report["diff"]["nodes"]["left_only"],
            json!([{"id":"node-1","kind":"event","ref_id":"node-1"}])
        );
        assert_eq!(
            report["diff"]["nodes"]["right_only"],
            json!([{"id":"node-3","kind":"event","ref_id":"node-3"}])
        );
        assert_eq!(
            report["diff"]["edges"]["left_only"],
            json!([{"from":"node-1","to":"node-2","relation":"causes"}])
        );
        assert_eq!(
            report["diff"]["edges"]["right_only"],
            json!([{"from":"node-2","to":"node-3","relation":"causes"}])
        );
    }

    #[test]
    fn print_json_report_emits_pretty_json() {
        use std::io::Write;

        let report = serde_json::json!({
            "ok": true,
            "schema_version": "cli-json-v1",
            "nested": { "a": 1, "b": [true, false] }
        });

        let expected = serde_json::to_string_pretty(&report).unwrap() + "\n";

        let mut buffer = Vec::new();
        {
            let mut writer = std::io::Cursor::new(&mut buffer);
            write!(
                &mut writer,
                "{}",
                serde_json::to_string_pretty(&report).unwrap()
            )
            .unwrap();
            writeln!(&mut writer).unwrap();
        }

        assert_eq!(String::from_utf8(buffer).unwrap(), expected);
    }
}
