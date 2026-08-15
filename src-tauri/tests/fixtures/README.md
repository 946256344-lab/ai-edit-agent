# Agent contract and regression fixtures

`agent_tool_contracts.v1.json` is a versioned inventory extracted from the
current `agentloop/policy.rs` implementation. It describes the whitelisted
observation, edit, and delivery skills. It is not a claim that a generated
JSON Schema or public error-code contract already exists.

`agent_regression_cases.v1.json` is the first behavior-oracle suite for the
high-risk natural-language and safety scenarios. The cases are intentionally
marked `fixture_only`: the parent `agentloop.rs` currently calls `ModelAccess` directly,
so the repository does not yet have a scripted provider seam that can execute
the complete multi-step transcripts deterministically.

Run the consistency checks with:

```powershell
cd src-tauri
cargo test --test agent_contract_assets
```

The test currently verifies:

- both fixtures parse as JSON;
- all required contract fields are present;
- contract names exactly match `OBSERVATION_TOOLS` plus `EDIT_TOOLS` in
  `agentloop/policy.rs`;
- scripted steps only use a whitelisted skill or control action;
- the requested regression-risk categories are represented.

The next automation step should provide a scripted decision seam around loop
steps, then execute each case against a temporary scoped SQLite database. That
runner should assert persisted versions and operation logs as well as returned
messages; it must not weaken the production Provider, scope, or artifact gates.
