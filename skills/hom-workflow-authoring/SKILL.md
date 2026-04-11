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
      <minijinja template — can reference top-level vars like {{ task }} and step outputs like {{ steps.plan.output }}>
    depends_on: [<step-id>, ...]     # Optional — steps with no deps run first
    timeout: <duration>              # Optional — e.g., "300s", "5m", or bare seconds like "300"
    condition: '<expression>'        # Optional — evaluated before execution
    retry:                           # Optional
      max_attempts: <n>
      backoff: exponential|linear|fixed
    on_failure: skip                 # Optional — default is abort
    # Or:
    # on_failure:
    #   fallback: <step-id>
```

## Validation Rules

The parser in `parser.rs` and `dag.rs` enforce:

1. **Unique step IDs** — No two steps can share an `id`
2. **Valid dependencies** — Every `depends_on` reference must point to an existing step `id`
3. **Acyclic graph** — `petgraph::algo::toposort` must succeed; cycles are rejected
4. **Valid harness names** — `harness` field must parse via `HarnessType::from_str_loose()`
5. **Timeout parsing** — `parse_timeout()` accepts `300s`, `5m`, or bare seconds like `300`

## Writing Good Workflows

### Step Prompts

Prompts are minijinja templates. Available variables:

- `{{ task }}` — Runtime variables from `--var key=value` are injected at the top level by name
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
- `expr1 && expr2`
- `expr1 || expr2`

Notes:
- `&&` binds tighter than `||`
- Parentheses are not supported

### Failure Handling

| Strategy | Behavior |
|----------|----------|
| `abort` (default) | Stop entire workflow, report failure |
| `skip` | Mark step as skipped, continue to dependents |
| `on_failure: { fallback: <step-id> }` | Run an alternative step instead |

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
        StepDef {
            id: "a".into(),
            harness: "claude".into(),
            model: None,
            prompt: "a".into(),
            depends_on: vec!["b".into()],
            timeout: None,
            condition: None,
            retry: None,
            on_failure: None,
        },
        StepDef {
            id: "b".into(),
            harness: "codex".into(),
            model: None,
            prompt: "b".into(),
            depends_on: vec!["a".into()],
            timeout: None,
            condition: None,
            retry: None,
            on_failure: None,
        },
    ];
    assert!(WorkflowDag::from_steps(&steps).is_err());
}

#[test]
fn test_topo_order() {
    let steps = vec![
        StepDef {
            id: "plan".into(),
            harness: "claude".into(),
            model: None,
            prompt: "plan".into(),
            depends_on: vec![],
            timeout: None,
            condition: None,
            retry: None,
            on_failure: None,
        },
        StepDef {
            id: "implement".into(),
            harness: "codex".into(),
            model: None,
            prompt: "implement".into(),
            depends_on: vec!["plan".into()],
            timeout: None,
            condition: None,
            retry: None,
            on_failure: None,
        },
    ];
    let dag = WorkflowDag::from_steps(&steps).unwrap();
    let order = dag.topo_order().unwrap();
    assert_eq!(order, vec!["plan".to_string(), "implement".to_string()]);
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
