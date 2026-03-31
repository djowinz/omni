//! Flat tree representation for CSS selector matching.
//!
//! Converts an HtmlNode tree into a Vec<FlatNode> where each node
//! has a parent index for O(1) ancestor lookups. This enables efficient
//! descendant selector matching regardless of tree depth.

use super::types::{ConditionalClass, HtmlNode};

/// A flattened node with parent reference for ancestor traversal.
#[derive(Debug, Clone)]
pub struct FlatNode {
    /// The tag name (e.g., "div", "span"). Empty for text nodes.
    pub tag: String,
    /// Element ID attribute.
    pub id: Option<String>,
    /// CSS classes.
    pub classes: Vec<String>,
    /// Inline style attribute (unparsed).
    pub inline_style: Option<String>,
    /// Conditional class bindings (`class:name="expr"`).
    pub conditional_classes: Vec<ConditionalClass>,
    /// Index of parent in the flat list. None for root.
    pub parent_index: Option<usize>,
    /// Nesting depth (0 for root).
    pub depth: usize,
    /// True if this is a text node.
    pub is_text: bool,
    /// Text content (for text nodes only).
    pub text_content: Option<String>,
    /// Indices of child nodes in the flat list.
    pub child_indices: Vec<usize>,
}

/// Flatten an HtmlNode tree into a Vec<FlatNode>.
pub fn flatten_tree(root: &HtmlNode) -> Vec<FlatNode> {
    let mut nodes = Vec::new();
    flatten_recursive(root, None, 0, &mut nodes);
    nodes
}

fn flatten_recursive(
    node: &HtmlNode,
    parent_index: Option<usize>,
    depth: usize,
    nodes: &mut Vec<FlatNode>,
) {
    let my_index = nodes.len();

    match node {
        HtmlNode::Element { tag, id, classes, inline_style, conditional_classes, children } => {
            nodes.push(FlatNode {
                tag: tag.clone(),
                id: id.clone(),
                classes: classes.clone(),
                inline_style: inline_style.clone(),
                conditional_classes: conditional_classes.clone(),
                parent_index,
                depth,
                is_text: false,
                text_content: None,
                child_indices: Vec::new(), // filled after children are added
            });

            // Record parent's child index
            if let Some(pi) = parent_index {
                nodes[pi].child_indices.push(my_index);
            }

            // Flatten children
            let mut child_indices = Vec::new();
            for child in children {
                let child_idx = nodes.len();
                flatten_recursive(child, Some(my_index), depth + 1, nodes);
                child_indices.push(child_idx);
            }

            // Update this node's child_indices
            nodes[my_index].child_indices = child_indices;
        }
        HtmlNode::Text { content } => {
            nodes.push(FlatNode {
                tag: String::new(),
                id: None,
                classes: Vec::new(),
                inline_style: None,
                conditional_classes: Vec::new(),
                parent_index,
                depth,
                is_text: true,
                text_content: Some(content.clone()),
                child_indices: Vec::new(),
            });

            if let Some(pi) = parent_index {
                nodes[pi].child_indices.push(my_index);
            }
        }
    }
}

/// Get the ancestor chain for a node (from immediate parent up to root).
/// Returns a list of indices from parent to root.
pub fn ancestor_chain(nodes: &[FlatNode], index: usize) -> Vec<usize> {
    let mut chain = Vec::new();
    let mut current = nodes[index].parent_index;
    while let Some(idx) = current {
        chain.push(idx);
        current = nodes[idx].parent_index;
    }
    chain
}

/// Check if any ancestor of the node at `index` has the given class.
pub fn has_ancestor_with_class(nodes: &[FlatNode], index: usize, class: &str) -> bool {
    let mut current = nodes[index].parent_index;
    while let Some(idx) = current {
        if nodes[idx].classes.iter().any(|c| c == class) {
            return true;
        }
        current = nodes[idx].parent_index;
    }
    false
}

/// Check if any ancestor of the node at `index` has the given ID.
pub fn has_ancestor_with_id(nodes: &[FlatNode], index: usize, id: &str) -> bool {
    let mut current = nodes[index].parent_index;
    while let Some(idx) = current {
        if nodes[idx].id.as_deref() == Some(id) {
            return true;
        }
        current = nodes[idx].parent_index;
    }
    false
}

/// Check if any ancestor of the node at `index` has the given tag.
pub fn has_ancestor_with_tag(nodes: &[FlatNode], index: usize, tag: &str) -> bool {
    let mut current = nodes[index].parent_index;
    while let Some(idx) = current {
        if nodes[idx].tag == tag {
            return true;
        }
        current = nodes[idx].parent_index;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tree() -> HtmlNode {
        // <div class="panel">
        //   <div class="row">
        //     <span class="value critical" id="cpu">text</span>
        //   </div>
        //   <span class="label">label</span>
        // </div>
        HtmlNode::Element {
            tag: "div".to_string(),
            id: None,
            classes: vec!["panel".to_string()],
            inline_style: None,
            conditional_classes: vec![],
            children: vec![
                HtmlNode::Element {
                    tag: "div".to_string(),
                    id: None,
                    classes: vec!["row".to_string()],
                    inline_style: None,
                    conditional_classes: vec![],
                    children: vec![
                        HtmlNode::Element {
                            tag: "span".to_string(),
                            id: Some("cpu".to_string()),
                            classes: vec!["value".to_string(), "critical".to_string()],
                            inline_style: Some("color: red;".to_string()),
                            conditional_classes: vec![],
                            children: vec![
                                HtmlNode::Text { content: "text".to_string() },
                            ],
                        },
                    ],
                },
                HtmlNode::Element {
                    tag: "span".to_string(),
                    id: None,
                    classes: vec!["label".to_string()],
                    inline_style: None,
                    conditional_classes: vec![],
                    children: vec![
                        HtmlNode::Text { content: "label".to_string() },
                    ],
                },
            ],
        }
    }

    #[test]
    fn flatten_produces_correct_count() {
        let tree = make_tree();
        let flat = flatten_tree(&tree);
        // div.panel > div.row > span#cpu > "text"
        //           > span.label > "label"
        // = 6 nodes total
        assert_eq!(flat.len(), 6);
    }

    #[test]
    fn parent_indices_correct() {
        let tree = make_tree();
        let flat = flatten_tree(&tree);

        // Root div.panel at index 0 — no parent
        assert_eq!(flat[0].parent_index, None);
        assert_eq!(flat[0].tag, "div");

        // div.row at index 1 — parent is 0
        assert_eq!(flat[1].parent_index, Some(0));

        // span#cpu at index 2 — parent is 1 (div.row)
        assert_eq!(flat[2].parent_index, Some(1));
        assert_eq!(flat[2].id.as_deref(), Some("cpu"));

        // "text" at index 3 — parent is 2 (span#cpu)
        assert!(flat[3].is_text);
        assert_eq!(flat[3].parent_index, Some(2));
    }

    #[test]
    fn ancestor_chain_correct() {
        let tree = make_tree();
        let flat = flatten_tree(&tree);

        // span#cpu (index 2) ancestors: div.row (1), div.panel (0)
        let chain = ancestor_chain(&flat, 2);
        assert_eq!(chain, vec![1, 0]);
    }

    #[test]
    fn ancestor_class_check() {
        let tree = make_tree();
        let flat = flatten_tree(&tree);

        // span#cpu (index 2) has ancestor with class "panel"
        assert!(has_ancestor_with_class(&flat, 2, "panel"));
        assert!(has_ancestor_with_class(&flat, 2, "row"));
        assert!(!has_ancestor_with_class(&flat, 2, "nonexistent"));
    }

    #[test]
    fn deeply_nested_tree() {
        // Create a 20-level deep tree
        let mut node = HtmlNode::Text { content: "deep".to_string() };
        for i in 0..20 {
            node = HtmlNode::Element {
                tag: "div".to_string(),
                id: None,
                classes: vec![format!("level-{}", i)],
                inline_style: None,
                conditional_classes: vec![],
                children: vec![node],
            };
        }

        let flat = flatten_tree(&node);
        assert_eq!(flat.len(), 21); // 20 divs + 1 text

        // Deepest node should have ancestor chain of length 20
        let chain = ancestor_chain(&flat, 20);
        assert_eq!(chain.len(), 20);
    }

    #[test]
    fn child_indices_correct() {
        let tree = make_tree();
        let flat = flatten_tree(&tree);

        // Root div.panel has 2 element children: div.row (1) and span.label (4)
        assert_eq!(flat[0].child_indices.len(), 2);
        assert_eq!(flat[0].child_indices[0], 1); // div.row
        assert_eq!(flat[0].child_indices[1], 4); // span.label
    }
}
