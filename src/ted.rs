use rowan::{Language, NodeOrToken, SyntaxElement, SyntaxNode};
use std::mem::Discriminant;

use crate::TreeEdit;
use itertools::Itertools;
use tree_edit_distance::{Edit, Node, Tree};

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

pub fn edits<'n, L>(from: &'n SyntaxNode<L>, to: &'n SyntaxNode<L>) -> Vec<TreeEdit>
where
    L: Language,
    L: 'static,
{
    let (edits, _) = tree_edit_distance::diff(&tree_node(from), &tree_node(to));
    generate_edit(&edits)
}

pub fn generate_edit(edits: &[Edit]) -> Vec<TreeEdit> {
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
