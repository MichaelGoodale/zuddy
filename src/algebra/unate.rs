use std::{
    cmp::Ordering::{Equal, Greater, Less},
    hash::Hash,
};

use crate::{Operations, SetFamily, Zdd, ZddHolder};

fn cmp_tops<V: Ord>(a: SetFamily<V>, b: SetFamily<V>, holder: &ZddHolder<V>) -> std::cmp::Ordering {
    match (a.0, b.0) {
        (a, b) if a == b => Equal,
        (1 | 0, 0 | 1) => Equal,
        (1 | 0, _) => Greater,
        (_, 0 | 1) => Less,
        (_, _) => {
            let (a, _, _) = a.get(holder).unwrap();
            let (b, _, _) = b.get(holder).unwrap();
            a.cmp(b)
        }
    }
}

impl<V: Hash + Ord + Eq + Clone> SetFamily<V> {
    ///# Panics
    ///May panic if `self` or `other` are undefined in the [`ZddHolder`].
    #[must_use]
    pub fn join(self, other: SetFamily<V>, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        if cmp_tops(self, other, holder) == Greater {
            return other.join(self, holder);
        }

        if other.is_zero() {
            return SetFamily::ZERO;
        }
        if other.is_one() {
            return self;
        }

        let op = Operations::Join(self, other);
        if let Some(r) = holder.cache.get(&op) {
            return *r;
        }

        let (value, self_lo, self_hi) = self.get(holder).expect("Invalid index!");
        let value = value.clone();
        let (other_v, mut other_lo, mut other_hi) = other.get(holder).expect("Invalid index!");

        if other_v > &value {
            other_lo = other;
            other_hi = SetFamily::ZERO;
        }
        let a = self_hi.join(other_hi, holder);
        let b = self_hi.join(other_lo, holder);
        let c = self_lo.join(other_hi, holder);
        let product = a.union(b, holder).union(c, holder);
        let v_product = holder.get_node(Zdd {
            value: value.clone(),
            lo: SetFamily::ZERO,
            hi: product,
        });

        let joined = v_product.union(self_lo.join(other_lo, holder), holder);

        holder.cache.insert(op, joined);

        joined
    }

    ///Does `self` / {`v`} in the unate cube algebra of Minato.
    ///Identical to [`SetFamily::onset`]
    #[must_use]
    pub fn element_division(self, value: V, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        self.onset(value, holder)
    }

    ///Does `self` % {`v`} in the unate cube algebra of Minato.
    ///Identical to [`SetFamily::offset`]
    #[must_use]
    pub fn element_remainder(self, value: V, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        self.offset(value, holder)
    }

    ///# Panics
    ///May panic if `self` or `other` are undefined in the [`ZddHolder`].
    #[must_use]
    pub fn divide(self, other: SetFamily<V>, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        if other.is_one() {
            return self;
        }

        if self.is_zero() || self.is_one() {
            return SetFamily::ZERO;
        }
        if self == other {
            return SetFamily::ONE;
        }

        let (value, other_lo, other_hi) =
            other.get(holder).expect("Can't divide by the empty set!");

        if other_lo.is_zero() && other_hi.is_one() {
            return self.element_division(value.clone(), holder);
        }

        let op = Operations::Division(self, other);
        if let Some(r) = holder.cache.get(&op) {
            return *r;
        }

        let value = value.clone();
        let r_lo = self.element_division(value.clone(), holder);

        let mut r = r_lo.divide(other_hi, holder);

        if !r.is_zero() && !other_lo.is_zero() {
            let r_h = self.element_remainder(value, holder);
            let r_l = r_h.divide(other_lo, holder);
            r = r_h.intersect(r_l, holder);
        }

        holder.cache.insert(op, r);

        r
    }
}

#[cfg(test)]
mod test {
    use crate::SetFamily;

    use crate::algebra::test_op;

    #[test]
    fn test_join() {
        for (a, b, res) in [
            ("ab b c", "ab  ", "ab abc b c"),
            ("a b", "d c", "ad bd ac bc"),
        ] {
            test_op(a, b, res, SetFamily::join, "*");
        }
    }

    #[test]
    fn test_divide() {
        for (a, b, res) in [("a  ", "a", " "), ("abc bc ac", "bc", "a  ")] {
            test_op(a, b, res, SetFamily::divide, "/");
        }
    }
}
