//! Condition evaluator for workflow step conditions.
//!
//! Supports:
//! - Simple expressions: `steps.plan.status == "completed"`
//! - Contains operator: `steps.test.output contains "PASS"`
//! - Compound conditions: `expr1 && expr2`, `expr1 || expr2`
//!
//! Operator precedence: `&&` binds tighter than `||`.
//! Parentheses are not supported.

use std::collections::HashMap;

/// Evaluate a condition expression against step outputs and statuses.
///
/// Supports `&&` (AND) and `||` (OR) compound operators.
/// `&&` binds tighter than `||` (standard precedence).
pub fn evaluate_condition(
    expr: &str,
    step_outputs: &HashMap<String, String>,
    step_statuses: &HashMap<String, String>,
) -> bool {
    let expr = expr.trim();
    if expr.is_empty() {
        return true;
    }

    // Split on `||` first (lower precedence), then `&&` within each clause.
    let or_clauses: Vec<&str> = split_outside_quotes(expr, "||");
    if or_clauses.len() > 1 {
        if or_clauses.iter().any(|clause| clause.trim().is_empty()) {
            return false;
        }
        return or_clauses
            .iter()
            .any(|clause| evaluate_condition(clause, step_outputs, step_statuses));
    }

    let and_clauses: Vec<&str> = split_outside_quotes(expr, "&&");
    if and_clauses.len() > 1 {
        if and_clauses.iter().any(|clause| clause.trim().is_empty()) {
            return false;
        }
        return and_clauses
            .iter()
            .all(|clause| evaluate_condition(clause, step_outputs, step_statuses));
    }

    // Single atomic condition
    evaluate_atomic(expr, step_outputs, step_statuses)
}

/// Evaluate a single atomic condition (no &&/||).
fn evaluate_atomic(
    expr: &str,
    step_outputs: &HashMap<String, String>,
    step_statuses: &HashMap<String, String>,
) -> bool {
    let expr = expr.trim();

    // Handle "contains" operator
    if let Some((lhs, rhs)) = expr.split_once(" contains ") {
        let lhs_val = resolve_value(lhs.trim(), step_outputs, step_statuses);
        let rhs_val = strip_quotes(rhs.trim());
        return lhs_val.contains(rhs_val);
    }

    // Handle "==" operator
    if let Some((lhs, rhs)) = expr.split_once("==") {
        let lhs_val = resolve_value(lhs.trim(), step_outputs, step_statuses);
        let rhs_val = strip_quotes(rhs.trim()).trim();
        return lhs_val.trim() == rhs_val;
    }

    // Handle "!=" operator
    if let Some((lhs, rhs)) = expr.split_once("!=") {
        let lhs_val = resolve_value(lhs.trim(), step_outputs, step_statuses);
        let rhs_val = strip_quotes(rhs.trim()).trim();
        return lhs_val.trim() != rhs_val;
    }

    // Default: treat as truthy/falsy
    !expr.is_empty() && expr != "false" && expr != "0"
}

fn strip_quotes(s: &str) -> &str {
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Split a string on a delimiter, but only outside of quoted strings.
fn split_outside_quotes<'a>(s: &'a str, delimiter: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut start = 0;
    let delim_len = delimiter.len();
    let bytes = s.as_bytes();

    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\'' && !in_double {
            in_single = !in_single;
            i += 1;
        } else if bytes[i] == b'"' && !in_single {
            in_double = !in_double;
            i += 1;
        } else if !in_single
            && !in_double
            && i + delim_len <= bytes.len()
            && &s[i..i + delim_len] == delimiter
        {
            parts.push(&s[start..i]);
            i += delim_len;
            start = i;
        } else {
            i += 1;
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Resolve a dotted path like `steps.plan.output` to a value.
fn resolve_value(
    path: &str,
    step_outputs: &HashMap<String, String>,
    step_statuses: &HashMap<String, String>,
) -> String {
    let parts: Vec<&str> = path.split('.').collect();

    if parts.len() == 3 && parts[0] == "steps" {
        let step_id = parts[1];
        match parts[2] {
            "output" => step_outputs.get(step_id).cloned().unwrap_or_default(),
            "status" => step_statuses.get(step_id).cloned().unwrap_or_default(),
            _ => String::new(),
        }
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains() {
        let mut outputs = HashMap::new();
        outputs.insert("test".to_string(), "All tests PASS".to_string());
        let statuses = HashMap::new();

        assert!(evaluate_condition(
            r#"steps.test.output contains "PASS""#,
            &outputs,
            &statuses,
        ));
    }

    #[test]
    fn test_equals() {
        let outputs = HashMap::new();
        let mut statuses = HashMap::new();
        statuses.insert("plan".to_string(), "completed".to_string());

        assert!(evaluate_condition(
            r#"steps.plan.status == "completed""#,
            &outputs,
            &statuses,
        ));
    }

    #[test]
    fn test_and_both_true() {
        let outputs = HashMap::new();
        let mut statuses = HashMap::new();
        statuses.insert("plan".to_string(), "completed".to_string());
        statuses.insert("test".to_string(), "completed".to_string());

        assert!(evaluate_condition(
            r#"steps.plan.status == "completed" && steps.test.status == "completed""#,
            &outputs,
            &statuses,
        ));
    }

    #[test]
    fn test_and_one_false() {
        let outputs = HashMap::new();
        let mut statuses = HashMap::new();
        statuses.insert("plan".to_string(), "completed".to_string());
        statuses.insert("test".to_string(), "failed".to_string());

        assert!(!evaluate_condition(
            r#"steps.plan.status == "completed" && steps.test.status == "completed""#,
            &outputs,
            &statuses,
        ));
    }

    #[test]
    fn test_or_one_true() {
        let outputs = HashMap::new();
        let mut statuses = HashMap::new();
        statuses.insert("plan".to_string(), "failed".to_string());
        statuses.insert("test".to_string(), "completed".to_string());

        assert!(evaluate_condition(
            r#"steps.plan.status == "completed" || steps.test.status == "completed""#,
            &outputs,
            &statuses,
        ));
    }

    #[test]
    fn test_or_both_false() {
        let outputs = HashMap::new();
        let mut statuses = HashMap::new();
        statuses.insert("plan".to_string(), "failed".to_string());
        statuses.insert("test".to_string(), "failed".to_string());

        assert!(!evaluate_condition(
            r#"steps.plan.status == "completed" || steps.test.status == "completed""#,
            &outputs,
            &statuses,
        ));
    }

    #[test]
    fn test_and_or_precedence() {
        // A || B && C should be A || (B && C)
        let outputs = HashMap::new();
        let mut statuses = HashMap::new();
        statuses.insert("a".to_string(), "completed".to_string());
        statuses.insert("b".to_string(), "failed".to_string());
        statuses.insert("c".to_string(), "failed".to_string());

        // A is true, B && C is false. A || (B && C) = true
        assert!(evaluate_condition(
            r#"steps.a.status == "completed" || steps.b.status == "completed" && steps.c.status == "completed""#,
            &outputs,
            &statuses,
        ));
    }

    #[test]
    fn quoted_single_strings_do_not_split_on_operators() {
        let mut outputs = HashMap::new();
        outputs.insert("plan".to_string(), "alpha && beta || gamma".to_string());
        let statuses = HashMap::new();

        assert!(evaluate_condition(
            "steps.plan.output contains '&& beta ||'",
            &outputs,
            &statuses,
        ));
    }

    #[test]
    fn degenerate_compound_inputs_are_false_except_empty_expr() {
        let outputs = HashMap::new();
        let statuses = HashMap::new();

        assert!(evaluate_condition("", &outputs, &statuses));
        assert!(!evaluate_condition("&&", &outputs, &statuses));
        assert!(!evaluate_condition("a && ", &outputs, &statuses));
        assert!(!evaluate_condition(" || ", &outputs, &statuses));
    }
}
