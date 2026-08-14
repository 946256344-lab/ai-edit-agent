use serde_json::Value;
use std::collections::BTreeSet;

const CONTRACT_FIXTURE: &str = include_str!("fixtures/agent_tool_contracts.v1.json");
const REGRESSION_FIXTURE: &str = include_str!("fixtures/agent_regression_cases.v1.json");
const AGENT_LOOP_SOURCE: &str = include_str!("../src/agentloop.rs");

fn parse_fixture(raw: &str, label: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|error| panic!("{label} must be valid JSON: {error}"))
}

fn required_string<'a>(value: &'a Value, field: &str, label: &str) -> &'a str {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| panic!("{label}.{field} must be a non-empty string"))
}

fn source_tool_names(constant_name: &str) -> BTreeSet<String> {
    let declaration = format!("const {constant_name}:");
    let start = AGENT_LOOP_SOURCE
        .find(&declaration)
        .unwrap_or_else(|| panic!("{constant_name} must exist in agentloop.rs"));
    let tail = &AGENT_LOOP_SOURCE[start..];
    let values_start = tail
        .find("= &[")
        .unwrap_or_else(|| panic!("{constant_name} must use an array literal"));
    let values = &tail[values_start + 4..];
    let values_end = values
        .find("];")
        .unwrap_or_else(|| panic!("{constant_name} array literal must terminate"));
    values[..values_end]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

#[test]
fn tool_contract_catalog_matches_the_agent_loop_whitelist() {
    let fixture = parse_fixture(CONTRACT_FIXTURE, "tool contract fixture");
    assert_eq!(
        fixture.get("catalogVersion").and_then(Value::as_u64),
        Some(1)
    );

    let tools = fixture
        .get("tools")
        .and_then(Value::as_array)
        .expect("tool contract fixture must contain a tools array");
    assert_eq!(
        tools.len(),
        21,
        "the current loop exposes exactly 21 skills"
    );

    let mut fixture_names = BTreeSet::new();
    for tool in tools {
        let name = required_string(tool, "name", "tool contract");
        assert!(
            fixture_names.insert(name.to_owned()),
            "tool contract names must be unique: {name}"
        );
        assert_eq!(
            tool.get("version").and_then(Value::as_u64),
            Some(1),
            "{name} must declare contract version 1"
        );
        assert!(
            matches!(
                required_string(tool, "kind", name),
                "observation" | "edit" | "delivery"
            ),
            "{name}.kind must be observation, edit, or delivery"
        );
        assert_eq!(
            tool.pointer("/inputShape/type").and_then(Value::as_str),
            Some("object"),
            "{name}.inputShape must describe a top-level object"
        );
        assert!(
            tool.get("scope").and_then(Value::as_object).is_some(),
            "{name}.scope must be an object"
        );
        assert!(
            tool.get("sideEffect").and_then(Value::as_bool).is_some(),
            "{name}.sideEffect must be a boolean"
        );
        required_string(tool, "idempotency", name);
        required_string(tool, "retryPolicy", name);
        assert!(
            tool.get("preconditions")
                .and_then(Value::as_array)
                .is_some(),
            "{name}.preconditions must be an array"
        );
        assert!(
            tool.get("produces").and_then(Value::as_array).is_some(),
            "{name}.produces must be an array"
        );
    }

    let mut source_names = source_tool_names("OBSERVATION_TOOLS");
    source_names.extend(source_tool_names("EDIT_TOOLS"));
    assert_eq!(
        fixture_names, source_names,
        "contract fixture must change whenever the agent-loop whitelist changes"
    );

    assert_eq!(
        fixture
            .pointer("/argumentConvention/location")
            .and_then(Value::as_str),
        Some("top_level")
    );
    assert_eq!(
        fixture
            .pointer("/argumentConvention/case")
            .and_then(Value::as_str),
        Some("camelCase")
    );
    let removed_meta_keys = fixture
        .pointer("/argumentConvention/removedMetaKeys")
        .and_then(Value::as_array)
        .expect("removedMetaKeys must be an array");
    assert!(removed_meta_keys.iter().any(|value| value == "goal"));
    assert!(removed_meta_keys.iter().any(|value| value == "isQuestion"));
    assert_eq!(
        fixture
            .pointer("/runtimePolicy/maximumSteps")
            .and_then(Value::as_u64),
        Some(10)
    );
}

#[test]
fn regression_fixture_covers_the_required_agent_risk_categories() {
    let fixture = parse_fixture(REGRESSION_FIXTURE, "agent regression fixture");
    assert_eq!(fixture.get("suiteVersion").and_then(Value::as_u64), Some(1));
    let cases = fixture
        .get("cases")
        .and_then(Value::as_array)
        .expect("agent regression fixture must contain a cases array");
    assert!(
        cases.len() >= 9,
        "the first suite must cover all requested risks"
    );

    let mut case_ids = BTreeSet::new();
    let mut covered_categories = BTreeSet::new();
    let mut allowed_steps = source_tool_names("OBSERVATION_TOOLS");
    allowed_steps.extend(source_tool_names("EDIT_TOOLS"));
    allowed_steps.extend(
        ["ask_user", "finish", "no_action", "done"]
            .into_iter()
            .map(str::to_owned),
    );

    for case in cases {
        let id = required_string(case, "id", "regression case");
        assert!(
            case_ids.insert(id.to_owned()),
            "regression case IDs must be unique: {id}"
        );
        required_string(case, "description", id);
        required_string(case, "automation", id);
        assert!(
            case.get("turns")
                .and_then(Value::as_array)
                .is_some_and(|turns| !turns.is_empty()),
            "{id}.turns must be a non-empty array"
        );
        assert!(
            case.get("expected").and_then(Value::as_object).is_some(),
            "{id}.expected must be an object"
        );
        let categories = case
            .get("categories")
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{id}.categories must be an array"));
        for category in categories {
            let category = category
                .as_str()
                .unwrap_or_else(|| panic!("{id} categories must be strings"));
            covered_categories.insert(category.to_owned());
        }

        if let Some(script) = case.get("providerScript").and_then(Value::as_array) {
            for step in script {
                let tool = required_string(step, "tool", id);
                assert!(
                    allowed_steps.contains(tool),
                    "{id} references unknown scripted tool {tool}"
                );
            }
        }
    }

    let required_categories: BTreeSet<String> = [
        "chinese_synonyms",
        "mixed_question_and_edit",
        "multi_turn_reference",
        "missing_assets",
        "malformed_json",
        "cross_scope",
        "premature_finish",
        "source_range_overflow",
        "clarification_required",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert!(
        required_categories.is_subset(&covered_categories),
        "regression fixture is missing required categories: {:?}",
        required_categories
            .difference(&covered_categories)
            .collect::<Vec<_>>()
    );
}
