//! Module for serializing/deserializing ZDDs with dedicated type that owns its data.

use std::{
    collections::{BTreeSet, HashMap},
    hash::{BuildHasher, Hash},
};

use crate::{SetFamily, ZddHolder, manager::ZddIndex};
use ahash::HashMapExt;
use serde::{Deserialize, Serialize, ser::SerializeStruct};

mod verification;
pub use verification::InvalidZdd;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
struct OwnedZddNode<T> {
    value: T,
    hi: usize,
    lo: usize,
}

///The index of a [`OwnedZDD`] in a [`MultipleOwnedZDD`]. Useful if you need to get specific members
///of a [`MultipleOwnedZDD`] or store them in a collection somehow.
#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OwnedZddIndex(usize);

///A ZDD which owns its own data. This is not useful for running algorithms over ZDDs, but can be
///helpful when saving or loading ZDDs.
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
#[serde(try_from = "UnverifiedOwnedZdd<T>")]
#[serde(bound(deserialize = "T: Ord + Deserialize<'de>"))]
pub struct OwnedZdd<T> {
    nodes: Vec<OwnedZddNode<T>>,
    root: OwnedZddIndex,
}

///A set of ZDDs which own their own data, like [`OwnedZDD`].
///By serializing a _set_ of ZDDs, we save a lot on serialization space.
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
#[serde(try_from = "UnverifiedOwnedZdd<T>")]
#[serde(bound(deserialize = "T: Ord + Deserialize<'de>"))]
pub struct MultipleOwnedZdd<T> {
    nodes: Vec<OwnedZddNode<T>>,
    roots: BTreeSet<OwnedZddIndex>,
}

impl<T> MultipleOwnedZdd<T> {
    ///Retuns a [`BTreeSet`] with the indices of all ZDD roots.
    ///May be useful in combination with [`MultipleOwnedZDD::to_set_families`]'s returned
    ///[`HashMap`].
    ///
    ///See [`to_owned_zdds_with_mapping`] to see how to make a [`MultipleOwnedZDD`] while keeping
    ///track of which Zdd is which.
    #[must_use]
    pub fn members(&self) -> &BTreeSet<OwnedZddIndex> {
        &self.roots
    }

    ///The number of Zdds roots in this [`MultipleOwnedZDD`].
    #[must_use]
    pub fn len(&self) -> usize {
        self.roots.len()
    }

    ///Check whether the [`MultipleOwnedZDD`] lacks any roots.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize)]
struct UnverifiedOwnedZdd<T> {
    nodes: Vec<OwnedZddNode<T>>,
    roots: BTreeSet<OwnedZddIndex>,
}

impl<T: Serialize> Serialize for MultipleOwnedZdd<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("UnverifiedOwnedZdd", 2)?;
        state.serialize_field("nodes", &self.nodes)?;
        state.serialize_field("roots", &self.roots)?;
        state.end()
    }
}

impl<T: Serialize> Serialize for OwnedZdd<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("UnverifiedOwnedZdd", 2)?;
        state.serialize_field("nodes", &self.nodes)?;
        state.serialize_field("roots", &BTreeSet::from([self.root]))?;
        state.end()
    }
}

enum DFSPortion<V> {
    Search(ZddIndex<V>),
    Add {
        id: ZddIndex<V>,
        value: V,
        lo: ZddIndex<V>,
        hi: ZddIndex<V>,
    },
}

impl<V> DFSPortion<V> {
    fn id(&self) -> &ZddIndex<V> {
        match self {
            DFSPortion::Search(zdd_index) | DFSPortion::Add { id: zdd_index, .. } => zdd_index,
        }
    }
}

// AsRef impl for to_owned_zdds below
impl<'a, V: Eq + Hash> AsRef<SetFamily<'a, V>> for SetFamily<'a, V> {
    fn as_ref(&self) -> &SetFamily<'a, V> {
        self
    }
}

///Converts a collection that implements [`IntoIterator`] to a [`MultipleOwnedZDD`].
///
///If you need to record info about each ZDD beyond having a set of ZDDs, see: [`MultipleOwnedZDD`].
#[must_use]
pub fn to_owned_zdds<'a, V, T, X>(zdds: T) -> MultipleOwnedZdd<V>
where
    V: Eq + Hash + Clone + 'a,
    X: AsRef<SetFamily<'a, V>>,
    T: IntoIterator<Item = X>,
{
    let (x, _) = to_owned_zdds_with_mapping::<_, _, _, ahash::RandomState>(zdds);
    x
}
impl<V: Eq + Hash + Clone> From<SetFamily<'_, V>> for OwnedZdd<V> {
    fn from(value: SetFamily<V>) -> Self {
        value.to_owned_zdd()
    }
}

impl<'a, V, S, C> From<C> for MultipleOwnedZdd<V>
where
    V: Eq + Hash + Clone + 'a,
    S: AsRef<SetFamily<'a, V>>,
    C: IntoIterator<Item = S>,
{
    fn from(value: C) -> Self {
        to_owned_zdds(value)
    }
}

///Converts a collection that implements [`IntoIterator`] to a [`MultipleOwnedZDD`] while returning
///a [`HashMap`] from the elements of the collection to their index in the [`MultipleOwnedZDD`].
///
///Useful if you need information connected to specific ZDDs beyond a set of ZDDs.
///
///# Panics
///Will panic if the ZDDs do not all share the same manager.
#[must_use]
pub fn to_owned_zdds_with_mapping<'a, V, T, X, S>(
    zdds: T,
) -> (
    MultipleOwnedZdd<V>,
    HashMap<SetFamily<'a, V>, OwnedZddIndex, S>,
)
where
    V: Eq + Hash + Clone + 'a,
    X: AsRef<SetFamily<'a, V>>,
    T: IntoIterator<Item = X>,
    S: Default + BuildHasher,
{
    let zdds = zdds.into_iter().collect::<Vec<_>>();
    if zdds.is_empty() {
        return (
            MultipleOwnedZdd {
                nodes: vec![],
                roots: BTreeSet::new(),
            },
            HashMap::default(),
        );
    }

    let mut nodes = vec![];

    let manager = zdds.first().unwrap().as_ref().manager();

    for x in zdds.iter().skip(1) {
        assert!(
            std::ptr::eq(x.as_ref().manager(), manager),
            "All ZDDs must have the same manager!"
        );
    }

    let mut stack = zdds
        .iter()
        .map(|zdd| DFSPortion::Search(zdd.as_ref().as_raw()))
        .collect::<Vec<_>>();

    let mut visited = ahash::HashMap::new();
    visited.insert(ZddIndex::ZERO, 0);
    visited.insert(ZddIndex::ONE, 1);

    while let Some(n) = stack.pop() {
        if visited.contains_key(n.id()) {
            continue;
        }

        match n {
            DFSPortion::Search(zdd_index) => {
                //since 1 and 0 are prefilled, we're guaranteed to never open them here.
                let (value, lo, hi) = zdd_index.get(manager).unwrap();

                stack.push(DFSPortion::Add {
                    id: zdd_index,
                    value,
                    lo,
                    hi,
                });

                if !visited.contains_key(&lo) {
                    stack.push(DFSPortion::Search(lo));
                }
                if !visited.contains_key(&hi) {
                    stack.push(DFSPortion::Search(hi));
                }
            }
            DFSPortion::Add { id, value, lo, hi } => {
                //Unwrap is fine here because we visit the children first in the stack
                let (lo, hi) = (*visited.get(&lo).unwrap(), *visited.get(&hi).unwrap());

                nodes.push(OwnedZddNode { value, hi, lo });
                visited.insert(id, nodes.len() - 1 + 2); //We add 2 to get the position,
                //assuming 1 and 0 are taken by ONE and ZERO.
            }
        }
    }

    let visited = zdds
        .iter()
        .map(|x| {
            (
                x.as_ref().clone(),
                OwnedZddIndex(*visited.get(&x.as_ref().as_raw()).unwrap()),
            )
        })
        .collect::<HashMap<_, _, S>>();

    (
        MultipleOwnedZdd {
            nodes,
            //We've built the values with the DFS loop, so unwrap is fine
            roots: visited.values().copied().collect(),
        },
        visited,
    )
}

impl<V: Eq + Hash + Clone> SetFamily<'_, V> {
    ///Convert this [`SetFamily`] to an [`OwnedZDD`] (useful for serialization)
    #[must_use]
    pub fn to_owned_zdd(&self) -> OwnedZdd<V> {
        let MultipleOwnedZdd {
            nodes,
            roots: mut root,
        } = to_owned_zdds(std::iter::once(self));
        OwnedZdd {
            nodes,
            #[expect(clippy::missing_panics_doc)] // will not panic since we have the one root
            // beforehand
            root: root.pop_first().unwrap(),
        }
    }
}

impl<V: Eq + Hash + Clone + Send + Sync> OwnedZdd<V> {
    ///Converts an [`OwnedZDD`] into a [`SetFamily`] associated with `holder`.
    pub fn to_set_family(self, holder: &ZddHolder<V>) -> SetFamily<'_, V> {
        let mut mapping = ahash::HashMap::new();
        mapping.insert(0, holder.zero());
        mapping.insert(1, holder.one());
        for (i, OwnedZddNode { value, hi, lo }) in self.nodes.into_iter().enumerate() {
            #[expect(clippy::missing_panics_doc)] //fine bc we serialize w/ children first.
            let (lo, hi) = (
                mapping.get(&lo).unwrap().clone(),
                mapping.get(&hi).unwrap().clone(),
            );
            let n = holder.get_node(value, lo, hi);
            mapping.insert(i + 2, n); //+2 to account for ONE and ZERO
        }

        //fine bc we've added the root when going over all members
        #[expect(clippy::missing_panics_doc)]
        mapping.get(&self.root.0).unwrap().clone()
    }
}

impl<V: Eq + Hash + Clone + Send + Sync> MultipleOwnedZdd<V> {
    ///Converts an [`MultipleOwnedZDD`] into a [`SetFamily`] associated with `holder`.
    pub fn to_set_families(
        self,
        holder: &ZddHolder<V>,
    ) -> HashMap<OwnedZddIndex, SetFamily<'_, V>> {
        let mut mapping = ahash::HashMap::new();
        mapping.insert(0, holder.zero());
        mapping.insert(1, holder.one());
        for (i, OwnedZddNode { value, hi, lo }) in self.nodes.into_iter().enumerate() {
            #[expect(clippy::missing_panics_doc)] //fine bc we serialize w/ children first.
            let (lo, hi) = (
                mapping.get(&lo).unwrap().clone(),
                mapping.get(&hi).unwrap().clone(),
            );
            let n = holder.get_node(value, lo, hi);
            mapping.insert(i + 2, n); //+2 to account for ONE and ZERO
        }

        //fine bc we've added the root when going over all members
        #[expect(clippy::missing_panics_doc)]
        self.roots
            .iter()
            .map(|x| (*x, mapping.get(&x.0).unwrap().clone()))
            .collect()
    }
}

#[cfg(test)]
mod test {
    use crate::{
        SetFamily, ZddHolder,
        serialize::{MultipleOwnedZdd, OwnedZdd, to_owned_zdds},
        utils::test::str_to_sets,
    };

    #[test]
    fn serialization_tests() -> anyhow::Result<()> {
        let sets = ["abdcd", " ", "ab ad ac af ghl", "a", "ab ad ef "];
        for s in sets {
            let sets = str_to_sets(s);
            let holder = ZddHolder::new();
            let sets = SetFamily::from_sets(sets, &holder);
            let owned = sets.to_owned_zdd();
            let owned_2: OwnedZdd<char> = sets.clone().into();
            assert_eq!(owned, owned_2);
            let serialized = ron::to_string(&owned)?;
            let deserialize: OwnedZdd<char> = ron::from_str(&serialized)?;
            let new_sets = deserialize.clone().to_set_family(&holder);
            assert_eq!(new_sets, sets);

            let new_holder = ZddHolder::new();
            let new_sets = deserialize.to_set_family(&new_holder);
            assert_eq!(sets.size(), new_sets.size());
        }

        let holder = ZddHolder::new();
        let mut all_sets = sets
            .iter()
            .map(|s| SetFamily::from_sets(str_to_sets(s), &holder))
            .collect::<Vec<_>>();

        let multiples = to_owned_zdds(&all_sets);
        let multiples_2: MultipleOwnedZdd<char> = all_sets.clone().into();
        assert_eq!(multiples, multiples_2);

        let serialized = ron::to_string(&multiples)?;
        assert!(ron::from_str::<OwnedZdd<char>>(&serialized).is_err());
        let deserialize: MultipleOwnedZdd<char> = ron::from_str(&serialized)?;
        let mut new_sets = deserialize
            .to_set_families(&holder)
            .into_values()
            .collect::<Vec<_>>();
        new_sets.sort();
        all_sets.sort();
        assert_eq!(new_sets, all_sets);

        Ok(())
    }
}
