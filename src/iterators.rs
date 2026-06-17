use crate::{SetFamily, manager::RawZdd};
use std::hash::Hash;

///A simple iterator over the members of the ZDD.
///May not be very memory efficient.
pub struct ZddIter<'a, V: Eq + Hash> {
    stack: Vec<(RawZdd<V>, Vec<V>)>,
    root: SetFamily<'a, V>,
}

impl<V: Eq + Clone + Hash> Iterator for ZddIter<'_, V> {
    type Item = Vec<V>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((node, current_set)) = self.stack.pop() {
            if node.is_zero() {
                continue;
            }
            if node.is_one() {
                return Some(current_set);
            }

            let (v, lo, hi) = node.get(self.root.manager).unwrap();

            if !lo.is_zero() {
                self.stack.push((lo, current_set.clone()));
            }
            if !hi.is_zero() {
                let mut hi_set = current_set;
                hi_set.push(v.clone());
                self.stack.push((hi, hi_set));
            }
        }
        None
    }
}

impl<'a, V: Eq + Hash> SetFamily<'a, V> {
    ///Returns a [`ZddIter`] to iterate over all the valid combinations in this family.
    #[must_use]
    pub fn members(&self) -> ZddIter<'a, V> {
        //We can use raws here since they will all be children of self.
        ZddIter {
            stack: vec![(self.as_raw(), Vec::new())],
            root: self.clone(),
        }
    }
}
