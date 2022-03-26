use std::mem::Discriminant;

use rowan::{Language, NodeOrToken, SyntaxElement, SyntaxNode};

use itertools::Itertools;
use tree_edit_distance::{Edit, Node, Tree};

#[derive(Debug, Hash, PartialEq, Eq)]
pub enum TreeDiffInsertPos<L: Language> {
    After(SyntaxElement<L>),
    AsFirstChild(SyntaxElement<L>),
}

#[derive(Debug)]
pub struct TreeDiff<L: Language> {
    pub replacements: Vec<(SyntaxElement<L>, SyntaxElement<L>)>,
    pub deletions: Vec<SyntaxElement<L>>,
    // the vec as well as the indexmap are both here to preserve order
    pub insertions: Vec<(TreeDiffInsertPos<L>, Vec<SyntaxElement<L>>)>,
}

/// Finds a (potentially minimal) diff, which, applied to `from`, will result in `to`.
///
/// Specifically, returns a structure that consists of a replacements, insertions and deletions
/// such that applying this map on `from` will result in `to`.
///
/// This function tries to find a fine-grained diff.
pub fn diff<L: Language + 'static>(from: &SyntaxNode<L>, to: &SyntaxNode<L>) -> TreeDiff<L> {
    let (edits, _) = tree_edit_distance::diff(&tree_node(from), &tree_node(to));
    let mut diff = TreeDiff {
        replacements: Vec::new(),
        insertions: Vec::new(),
        deletions: Vec::new(),
    };
    generate_diff(
        &mut diff,
        generate_edit(&edits),
        None,
        Some(from.clone().into()).into_iter(),
        Some(to.clone().into()).into_iter(),
    );

    diff
}

#[derive(Debug)]
struct TreeNode<L: Language>(TreeNodeKind<L>, Vec<TreeNode<L>>);

#[derive(Debug)]
enum TreeNodeKind<L: Language> {
    Node(Discriminant<L::Kind>),
    Token(String),
}

use std::mem::discriminant;

impl<'n, L: Language> PartialEq for TreeNodeKind<L> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Node(l0), Self::Node(r0)) => l0 == r0,
            (Self::Token(l0), Self::Token(r0)) => l0 == r0,
            _ => false,
        }
    }
}
impl<'n, L: Language + 'static> Node<'n> for TreeNode<L> {
    type Kind = &'n TreeNodeKind<L>;
    fn kind(&'n self) -> Self::Kind {
        &self.0
    }

    type Weight = u32;
    fn weight(&'n self) -> Self::Weight {
        1
    }
}

impl<'t, L: Language + 'static> Tree<'t> for TreeNode<L> {
    type Children = std::slice::Iter<'t, TreeNode<L>>;
    fn children(&'t self) -> Self::Children {
        self.1.iter()
    }
}

fn tree_node<'n, L: Language + 'n>(elt: &SyntaxNode<L>) -> TreeNode<L> {
    TreeNode(
        TreeNodeKind::Node(discriminant(&elt.kind())),
        elt.children_with_tokens()
            .map(|c| tree_element(&c))
            .collect::<Vec<_>>(),
    )
}

fn tree_element<'n, L: Language + 'n>(elt: &SyntaxElement<L>) -> TreeNode<L> {
    match elt {
        NodeOrToken::Node(node) => tree_node(node),
        NodeOrToken::Token(token) => TreeNode(TreeNodeKind::Token(token.to_string()), vec![]),
    }
}

#[derive(Debug, Clone)]
enum TreeEdit {
    Same,
    InsertFirst(usize),
    Insert(usize),
    Remove,
    Replace(Vec<TreeEdit>),
    RemoveInsert,
}

fn generate_edit(edits: &[Edit]) -> Vec<TreeEdit> {
    edits
        .iter()
        .map(|n| match n {
            Edit::Insert => TreeEdit::Insert(1),
            Edit::Remove => TreeEdit::Remove,
            Edit::Replace(ledits) => {
                let ledits = generate_edit(ledits);
                if ledits.is_empty() || ledits.iter().all(|e| matches!(e, TreeEdit::Same)) {
                    TreeEdit::Same
                } else {
                    TreeEdit::Replace(ledits)
                }
            }
        })
        .coalesce(|a, b| match (&a, &b) {
            (TreeEdit::Remove, TreeEdit::Insert(_)) => Ok(TreeEdit::RemoveInsert),
            (TreeEdit::Insert(_), TreeEdit::Remove) => Ok(TreeEdit::RemoveInsert),
            _ => Err((a, b)),
        })
        .group_by(|e| matches!(e, TreeEdit::Insert(_)))
        .into_iter()
        .flat_map(|(is_insert, group)| {
            if is_insert {
                vec![TreeEdit::Insert(group.count())]
            } else {
                group.into_iter().collect_vec()
            }
        })
        .enumerate()
        // insert first
        .map(|(i, d)| match (i, d) {
            (0, TreeEdit::Insert(i)) => TreeEdit::InsertFirst(i),
            (_, a) => a,
        })
        .collect_vec()
}

fn generate_diff<L: Language>(
    diff: &mut TreeDiff<L>,
    edits: Vec<TreeEdit>,
    left_parent: Option<SyntaxNode<L>>,
    mut left_childs: impl Iterator<Item = SyntaxElement<L>>,
    mut right_childs: impl Iterator<Item = SyntaxElement<L>>,
) {
    let mut current_left: Option<SyntaxElement<L>> = None;

    for edit in edits.into_iter() {
        match edit {
            TreeEdit::RemoveInsert => {
                current_left = left_childs.next();
                diff.replacements
                    .push((current_left.clone().unwrap(), right_childs.next().unwrap()));
            }
            TreeEdit::Insert(i) => {
                let pos = TreeDiffInsertPos::After(current_left.clone().unwrap());
                diff.insertions.push((
                    pos,
                    (0..i)
                        .into_iter()
                        .filter_map(|_| right_childs.next())
                        .collect_vec(),
                ));
            }
            TreeEdit::InsertFirst(i) => {
                let pos = TreeDiffInsertPos::AsFirstChild(NodeOrToken::Node(
                    left_parent.clone().unwrap(),
                ));
                diff.insertions.push((
                    pos,
                    (0..i)
                        .into_iter()
                        .filter_map(|_| right_childs.next())
                        .collect_vec(),
                ));
            }
            TreeEdit::Remove => {
                current_left = left_childs.next();
                diff.deletions.push(current_left.clone().unwrap());
            }
            TreeEdit::Replace(edits) => {
                current_left = left_childs.next();
                let left_parent = current_left.clone().and_then(|f| f.into_node());
                generate_diff(
                    diff,
                    edits,
                    left_parent.clone(),
                    left_parent
                        .clone()
                        .map(|f| f.children_with_tokens())
                        .unwrap(),
                    right_childs
                        .next()
                        .and_then(|f| f.into_node())
                        .map(|f| f.children_with_tokens())
                        .unwrap(),
                );
            }
            TreeEdit::Same => {
                current_left = left_childs.next();
                right_childs.next();
            }
        }
    }
}
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
