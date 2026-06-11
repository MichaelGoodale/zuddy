use super::{SetFamily, ZddHolder};

///A simple iterator over the members of the ZDD.
///May not be very memory efficient.
pub struct ZDDIter<'a, V> {
    stack: Vec<(SetFamily<V>, Vec<V>)>,
    holder: &'a ZddHolder<V>,
}

impl<V: Eq + Clone> Iterator for ZDDIter<'_, V> {
    type Item = Vec<V>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((node, current_set)) = self.stack.pop() {
            if node.is_zero() {
                continue;
            }
            if node.is_one() {
                return Some(current_set);
            }

            let (v, lo, hi) = node.get(self.holder).unwrap();

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

impl<V> SetFamily<V> {
    ///Returns a [`ZDDIter`] to iterate over all the valid combinations in this family.
    #[must_use]
    pub fn members(self, holder: &ZddHolder<V>) -> ZDDIter<'_, V> {
        ZDDIter {
            stack: vec![(self, Vec::new())],
            holder,
        }
    }
}
