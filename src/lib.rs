mod ted;
use rowan::{Language, SyntaxElement, SyntaxNode};

use itertools::Itertools;

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
    let mut diff = TreeDiff {
        replacements: Vec::new(),
        insertions: Vec::new(),
        deletions: Vec::new(),
    };
    generate_diff(
        &mut diff,
        ted::edits(from, to),
        None,
        Some(from.clone().into()).into_iter(),
        Some(to.clone().into()).into_iter(),
    );

    diff
}

#[derive(Debug, Clone)]
pub enum TreeEdit {
    Same,
    InsertFirst(usize),
    Insert(usize),
    Remove,
    Replace(Vec<TreeEdit>),
    RemoveInsert,
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
                let pos = TreeDiffInsertPos::AsFirstChild(left_parent.clone().unwrap().into());
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
