//! Reactive class evaluation — computes active classes per element each frame.

use omni_shared::SensorSnapshot;
use super::expression;
use super::flat_tree::FlatNode;

/// Evaluate all conditional classes for a flat node and return the full active class list
/// (static classes + conditionally active classes).
pub fn resolve_active_classes(node: &FlatNode, snapshot: &SensorSnapshot) -> Vec<String> {
    let mut active = node.classes.clone();

    for cc in &node.conditional_classes {
        if expression::eval_condition(&cc.expression, snapshot) {
            if !active.contains(&cc.class_name) {
                active.push(cc.class_name.clone());
            }
        }
        // If condition is false, the class is NOT in the list
        // (it was only in `active` if it was also a static class)
    }

    active
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::ConditionalClass;
    use omni_shared::SensorSnapshot;

    fn make_snapshot_with_gpu_temp(temp: f32) -> SensorSnapshot {
        let mut s = SensorSnapshot::default();
        s.gpu.temp_c = temp;
        s
    }

    fn make_snapshot_with_gpu_and_cpu(gpu_temp: f32, gpu_usage: f32, cpu_usage: f32) -> SensorSnapshot {
        let mut s = SensorSnapshot::default();
        s.gpu.temp_c = gpu_temp;
        s.gpu.usage_percent = gpu_usage;
        s.cpu.total_usage_percent = cpu_usage;
        s
    }

    fn make_node(classes: Vec<&str>, conditional_classes: Vec<ConditionalClass>) -> FlatNode {
        FlatNode {
            tag: "div".to_string(),
            id: None,
            classes: classes.into_iter().map(|s| s.to_string()).collect(),
            inline_style: None,
            conditional_classes,
            parent_index: None,
            depth: 0,
            is_text: false,
            text_content: None,
            child_indices: vec![],
        }
    }

    fn cc(class_name: &str, expression: &str) -> ConditionalClass {
        ConditionalClass {
            class_name: class_name.to_string(),
            expression: expression.to_string(),
        }
    }

    /// Test 1: No conditional classes → returns static classes only
    #[test]
    fn no_conditional_classes_returns_static_only() {
        let snapshot = SensorSnapshot::default();
        let node = make_node(vec!["panel", "row"], vec![]);
        let result = resolve_active_classes(&node, &snapshot);
        assert_eq!(result, vec!["panel".to_string(), "row".to_string()]);
    }

    /// Test 2: Condition true → class added
    #[test]
    fn condition_true_adds_class() {
        let snapshot = make_snapshot_with_gpu_temp(90.0);
        let node = make_node(
            vec!["panel"],
            vec![cc("warning", "gpu.temp > 80")],
        );
        let result = resolve_active_classes(&node, &snapshot);
        assert!(result.contains(&"panel".to_string()));
        assert!(result.contains(&"warning".to_string()));
        assert_eq!(result.len(), 2);
    }

    /// Test 3: Condition false → class NOT added
    #[test]
    fn condition_false_does_not_add_class() {
        let snapshot = make_snapshot_with_gpu_temp(70.0);
        let node = make_node(
            vec!["panel"],
            vec![cc("warning", "gpu.temp > 80")],
        );
        let result = resolve_active_classes(&node, &snapshot);
        assert_eq!(result, vec!["panel".to_string()]);
        assert!(!result.contains(&"warning".to_string()));
    }

    /// Test 4: Multiple conditions, some true some false → correct subset
    #[test]
    fn multiple_conditions_correct_subset() {
        let snapshot = make_snapshot_with_gpu_temp(88.0);
        let node = make_node(
            vec!["base"],
            vec![
                cc("hot", "gpu.temp > 70"),      // true  (88 > 70)
                cc("warning", "gpu.temp > 80"),  // true  (88 > 80)
                cc("critical", "gpu.temp > 95"), // false (88 <= 95)
            ],
        );
        let result = resolve_active_classes(&node, &snapshot);
        assert!(result.contains(&"base".to_string()));
        assert!(result.contains(&"hot".to_string()));
        assert!(result.contains(&"warning".to_string()));
        assert!(!result.contains(&"critical".to_string()));
        assert_eq!(result.len(), 3);
    }

    /// Test 5: Static class + same conditional class name → no duplicates
    #[test]
    fn no_duplicates_when_static_and_conditional_match() {
        let snapshot = make_snapshot_with_gpu_temp(90.0);
        let node = make_node(
            vec!["panel", "warning"],  // "warning" already static
            vec![cc("warning", "gpu.temp > 80")],  // condition also true
        );
        let result = resolve_active_classes(&node, &snapshot);
        // "warning" should appear exactly once
        let warning_count = result.iter().filter(|c| c.as_str() == "warning").count();
        assert_eq!(warning_count, 1);
        assert_eq!(result.len(), 2); // "panel" + "warning"
    }

    /// Test 6: Empty node (no static classes, no conditional classes)
    #[test]
    fn empty_node_returns_empty() {
        let snapshot = SensorSnapshot::default();
        let node = make_node(vec![], vec![]);
        let result = resolve_active_classes(&node, &snapshot);
        assert!(result.is_empty());
    }

    /// Test 7: Multiple conditions all false → only static classes remain
    #[test]
    fn all_conditions_false_returns_static_only() {
        let snapshot = make_snapshot_with_gpu_temp(50.0);
        let node = make_node(
            vec!["base"],
            vec![
                cc("hot", "gpu.temp > 70"),
                cc("warning", "gpu.temp > 80"),
                cc("critical", "gpu.temp > 95"),
            ],
        );
        let result = resolve_active_classes(&node, &snapshot);
        assert_eq!(result, vec!["base".to_string()]);
    }

    /// Test 8: Condition using AND expression — both conditions must be true
    #[test]
    fn condition_with_and_expression() {
        let snapshot = make_snapshot_with_gpu_and_cpu(88.0, 92.0, 95.0);
        let node = make_node(
            vec!["status"],
            vec![cc("overloaded", "gpu.temp > 80 && cpu.usage > 90")],
        );
        let result = resolve_active_classes(&node, &snapshot);
        assert!(result.contains(&"overloaded".to_string()));

        // When CPU usage is low, condition should be false
        let snapshot2 = make_snapshot_with_gpu_and_cpu(88.0, 92.0, 50.0);
        let node2 = make_node(
            vec!["status"],
            vec![cc("overloaded", "gpu.temp > 80 && cpu.usage > 90")],
        );
        let result2 = resolve_active_classes(&node2, &snapshot2);
        assert!(!result2.contains(&"overloaded".to_string()));
    }
}
