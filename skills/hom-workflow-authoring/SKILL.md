---
name: hom-workflow-authoring
description: Use when creating, modifying, or debugging YAML workflow definitions or the workflow engine
---

# HOM Workflow Authoring

## When to Use

Invoke this skill when:
- Creating a new YAML workflow template in `workflows/`
- Modifying the workflow parser in `crates/hom-workflow/src/parser.rs`
- Changing DAG construction in `crates/hom-workflow/src/dag.rs`
- Modifying the executor in `crates/hom-workflow/src/executor.rs`
- Adding new condition operators in `crates/hom-workflow/src/condition.rs`
- Working on checkpointing in `crates/hom-workflow/src/checkpoint.rs`

## Workflow YAML Schema

Every workflow file lives in `workflows/` or `~/.config/hom/workflows/` and must follow this schema:

```yaml
name: <unique-kebab-case-name>
description: <one-line description>

variables:
  <key>: <default-value-or-empty-string>

steps:
  - id: <unique-step-id>
    harness: <harness-name>          # Must match HarnessType::from_str_loose()
    model: <optional-model-name>
    prompt: |
      <minijinja template — can reference {{ variables }} and {{ steps.<id>.output }}>
    depends_on: [<step-id>, ...]     # Optional — steps with no deps run first
    timeout: <duration>              # Optional — e.g., "300s", "5m"
    condition: '<expression>'        # Optional — evaluated before execution
    retry:                           # Optional
      max_attempts: <n>
      backoff: exponential|linear|fixed
    on_failure: abort|skip|fallback(<step-id>)  # Optional — default is abort
```

## Validation Rules

The parser in `parser.rs` and `dag.rs` enforce:

1. **Unique step IDs** — No two steps can share an `id`
2. **Valid dependencies** — Every `depends_on` reference must point to an existing step `id`
3. **Acyclic graph** — `petgraph::algo::toposort` must succeed; cycles are rejected
4. **Valid harness names** — `harness` field must parse via `HarnessType::from_str_loose()`
5. **Timeout format** — Must end with `s` (seconds) or `m` (minutes)

## Writing Good Workflows

### Step Prompts

Prompts are minijinja templates. Available variables:

- `{{ variable_name }}` — Runtime variables from `--var key=value`
- `{{ steps.<step-id>.output }}` — Output captured from a completed step

**Good prompt:**
```yaml
prompt: |
  Review this codebase and create a detailed implementation plan for:
  {{ task }}
  Output a numbered list of steps with file paths and changes needed.
```

**Bad prompt:**
```yaml
prompt: "do the thing"  # Too vague — harness won't know what to do
```

### Dependency Design

Think of the DAG as a data flow graph. Each step should:
- Declare dependencies on steps whose **output** it needs
- Steps with no shared data can run in parallel (no `depends_on`)

```
plan ──→ implement ──→ validate ──→ security-review
                                         ↑
                                    (condition: PASS)
```

### Conditions

Conditions are evaluated before a step runs. If false, the step is skipped.

Supported operators:
- `steps.<id>.output contains "<substring>"`
- `steps.<id>.status == "completed"`
- `steps.<id>.status != "failed"`

### Failure Handling

| Strategy | Behavior |
|----------|----------|
| `abort` (default) | Stop entire workflow, report failure |
| `skip` | Mark step as skipped, continue to dependents |
| `fallback(<step-id>)` | Run an alternative step instead |

## Testing Workflows

### Unit tests for parser:
```rust
#[test]
fn test_parse_valid_workflow() {
    let yaml = r#"
name: test-workflow
description: A test
steps:
  - id: step1
    harness: claude-code
    prompt: "hello"
"#;
    let def = WorkflowDef::from_yaml(yaml).unwrap();
    assert_eq!(def.name, "test-workflow");
    assert_eq!(def.steps.len(), 1);
}

#[test]
fn test_reject_duplicate_step_ids() {
    let yaml = r#"
name: bad
steps:
  - id: dup
    harness: claude-code
    prompt: "a"
  - id: dup
    harness: codex
    prompt: "b"
"#;
    let def = WorkflowDef::from_yaml(yaml).unwrap();
    assert!(def.validate().is_err());
}
```

### Unit tests for DAG:
```rust
#[test]
fn test_cycle_detection() {
    let steps = vec![
        StepDef { id: "a".into(), depends_on: vec!["b".into()], .. },
        StepDef { id: "b".into(), depends_on: vec!["a".into()], .. },
    ];
    assert!(WorkflowDag::from_steps(&steps).is_err());
}

#[test]
fn test_topo_order() {
    let steps = vec![
        StepDef { id: "plan".into(), depends_on: vec![], .. },
        StepDef { id: "implement".into(), depends_on: vec!["plan".into()], .. },
    ];
    let dag = WorkflowDag::from_steps(&steps).unwrap();
    let order = dag.topo_order().unwrap();
    assert_eq!(order, vec!["plan", "implement"]);
}
```

### Unit tests for conditions:
```rust
#[test]
fn test_contains_condition() {
    let mut outputs = HashMap::new();
    outputs.insert("test".to_string(), "All 42 tests PASS".to_string());
    assert!(evaluate_condition(
        r#"steps.test.output contains "PASS""#,
        &outputs, &HashMap::new()
    ));
}
```

## Checklist Before Committing

- [ ] YAML workflow parses without error: `WorkflowDef::from_yaml()`
- [ ] Validation passes: `def.validate()`
- [ ] DAG builds without cycles: `WorkflowDag::from_steps()`
- [ ] All step harness names resolve via `HarnessType::from_str_loose()`
- [ ] Template variables are documented in `variables:` section
- [ ] `cargo test -p hom-workflow` passes
- [ ] Manual review: does the DAG make logical sense? (draw it out)
