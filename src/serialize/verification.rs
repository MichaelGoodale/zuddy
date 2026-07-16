use super::{MultipleOwnedZdd, OwnedZdd, OwnedZddIndex, OwnedZddNode, UnverifiedOwnedZdd};
use ahash::HashSetExt;
use thiserror::Error;

///An error triggered by a improper ZDD represented by [`UnverifiedOwnedZdd`].
#[derive(Debug, Copy, Clone, Error)]
pub enum InvalidZdd {
    ///Trying to convert to [`OwnedZdd`] when there are multiple roots (try [`MultipleOwnedZDD`]).
    #[error("This contains multiple ZDDs not just one!")]
    NotSingle,
    ///This violates the order for a ZDD with children having a lower value than their parents.
    #[error("This has values that are lower than its children!")]
    BadValueOrder,
    ///To make serialization faster, all children must be stored before their parents.
    #[error("This has children come after parents!")]
    BadSerialOrder,
    ///There cannot be any unused nodes in the [`UnverifiedOwnedZdd`].
    #[error("There are nodes that are dangling!")]
    UnvisitedNodes,
}

fn check_validity<T: Ord>(
    nodes: &[OwnedZddNode<T>],
    roots: impl Iterator<Item = OwnedZddIndex>,
) -> Result<(), InvalidZdd> {
    let mut visited = ahash::HashSet::new();
    visited.extend([0, 1]);
    visited.extend(roots.map(|x| x.0));
    let mut stack = visited
        .iter()
        .copied()
        .filter(|x| *x >= 2)
        .collect::<Vec<_>>();

    while let Some(idx) = stack.pop() {
        let OwnedZddNode { ref value, hi, lo } = nodes[idx - 2];

        if idx < lo || idx < hi {
            return Err(InvalidZdd::BadSerialOrder);
        }
        if lo >= 2 {
            let lo_v = &nodes[lo - 2].value;
            if lo_v <= value {
                return Err(InvalidZdd::BadValueOrder);
            }
            if !visited.contains(&lo) {
                stack.push(lo);
            }
        }

        if hi >= 2 {
            let hi_v = &nodes[hi - 2].value;
            if hi_v <= value {
                return Err(InvalidZdd::BadValueOrder);
            }
            if !visited.contains(&hi) {
                stack.push(hi);
            }
        }
        visited.insert(idx);
    }
    if visited.len() - 2 != nodes.len() {
        return Err(InvalidZdd::UnvisitedNodes);
    }
    Ok(())
}

impl<T: Ord> TryFrom<UnverifiedOwnedZdd<T>> for OwnedZdd<T> {
    type Error = InvalidZdd;

    fn try_from(value: UnverifiedOwnedZdd<T>) -> Result<Self, Self::Error> {
        let UnverifiedOwnedZdd { nodes, mut roots } = value;
        if roots.len() != 1 {
            return Err(InvalidZdd::NotSingle);
        }
        let root = roots.pop_first().unwrap();
        check_validity(&nodes, std::iter::once(root))?;

        Ok(OwnedZdd { nodes, root })
    }
}

impl<T: Ord> TryFrom<UnverifiedOwnedZdd<T>> for MultipleOwnedZdd<T> {
    type Error = InvalidZdd;

    fn try_from(value: UnverifiedOwnedZdd<T>) -> Result<Self, Self::Error> {
        let UnverifiedOwnedZdd { nodes, roots } = value;
        check_validity(&nodes, roots.iter().copied())?;
        Ok(MultipleOwnedZdd { nodes, roots })
    }
}
