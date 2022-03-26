use std::{hash::BuildHasherDefault, mem::Discriminant};

use rowan::{Language, NodeOrToken, SyntaxElement, SyntaxNode};

use indexmap::IndexMap;
use itertools::Itertools;
use rustc_hash::FxHashMap;
use tree_edit_distance::{Edit, Node, Tree};

type FxIndexMap<K, V> = IndexMap<K, V, BuildHasherDefault<rustc_hash::FxHasher>>;

#[derive(Debug, Hash, PartialEq, Eq)]
pub enum TreeDiffInsertPos<L: Language> {
    After(SyntaxElement<L>),
    AsFirstChild(SyntaxElement<L>),
}

#[derive(Debug)]
pub struct TreeDiff<L: Language> {
    pub replacements: FxHashMap<SyntaxElement<L>, SyntaxElement<L>>,
    pub deletions: Vec<SyntaxElement<L>>,
    // the vec as well as the indexmap are both here to preserve order
    pub insertions: FxIndexMap<TreeDiffInsertPos<L>, Vec<SyntaxElement<L>>>,
}

/// Finds a (potentially minimal) diff, which, applied to `from`, will result in `to`.
///
/// Specifically, returns a structure that consists of a replacements, insertions and deletions
/// such that applying this map on `from` will result in `to`.
///
/// This function tries to find a fine-grained diff.
pub fn diff<L: Language + 'static>(from: &SyntaxNode<L>, to: &SyntaxNode<L>) -> TreeDiff<L> {
    let mut diff = TreeDiff {
        replacements: FxHashMap::default(),
        insertions: FxIndexMap::default(),
        deletions: Vec::new(),
    };
    let (from, to) = (from.clone().into(), to.clone().into());
    let f = tree_node(&from);
    let t = tree_node(&to);
    let (edits, _) = tree_edit_distance::diff(&f, &t);
    generate_diff(
        &mut diff,
        generate_edit(&edits),
        None,
        Some(from.clone()).into_iter(),
        Some(to.clone()).into_iter(),
    );
    return diff;

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

    fn tree_node<'n, L: Language + 'n>(elt: &SyntaxElement<L>) -> TreeNode<L> {
        if let Some(node) = elt.as_node() {
            TreeNode(
                TreeNodeKind::Node(discriminant(&node.kind())),
                node.children_with_tokens()
                    .map(|c| tree_node(&c))
                    .collect::<Vec<_>>(),
            )
        } else {
            TreeNode(TreeNodeKind::Token(elt.to_string()), vec![])
        }
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
    let ret = edits
        .iter()
        .map(|n| match n {
            Edit::Insert => TreeEdit::Insert(1),
            Edit::Remove => TreeEdit::Remove,
            Edit::Replace(ledits) => {
                let ledits = generate_edit(ledits);
                if ledits.is_empty() {
                    TreeEdit::Same
                } else {
                    if ledits.iter().all(|e| match e {
                        TreeEdit::Same => true,
                        _ => false,
                    }) {
                        TreeEdit::Same
                    } else {
                        TreeEdit::Replace(ledits)
                    }
                }
            }
        })
        .coalesce(|a, b| match (&a, &b) {
            (TreeEdit::Remove, TreeEdit::Insert(_)) => Ok(TreeEdit::RemoveInsert),
            (TreeEdit::Insert(_), TreeEdit::Remove) => Ok(TreeEdit::RemoveInsert),
            _ => Err((a, b)),
        })
        .group_by(|e| match e {
            TreeEdit::Insert(_) => true,
            _ => false,
        })
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
        .collect_vec();
    return ret;
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
                    .insert(current_left.clone().unwrap(), right_childs.next().unwrap());
            }
            TreeEdit::Insert(i) => {
                let pos = TreeDiffInsertPos::After(current_left.clone().unwrap());
                diff.insertions.insert(
                    pos,
                    (0..i)
                        .into_iter()
                        .map(|_| right_childs.next())
                        .flatten()
                        .collect_vec(),
                );
            }
            TreeEdit::InsertFirst(i) => {
                let pos = TreeDiffInsertPos::AsFirstChild(NodeOrToken::Node(
                    left_parent.clone().unwrap(),
                ));
                diff.insertions.insert(
                    pos,
                    (0..i)
                        .into_iter()
                        .map(|_| right_childs.next())
                        .flatten()
                        .collect_vec(),
                );
            }
            TreeEdit::Remove => {
                current_left = left_childs.next();
                diff.deletions.push(current_left.clone().unwrap());
            }
            TreeEdit::Replace(edits) => {
                current_left = left_childs.next();
                let left_parent = current_left.clone().map(|f| f.into_node()).flatten();
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
                        .map(|f| f.into_node())
                        .flatten()
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