use std::{
    cmp::Ordering::{Equal, Greater, Less},
    hash::Hash,
};

use crate::{Operations, RawZdd, Zdd, ZddHolder};

fn cmp_tops<V: Ord + Hash + Eq + Clone>(
    a: RawZdd<V>,
    b: RawZdd<V>,
    holder: &ZddHolder<V>,
) -> std::cmp::Ordering {
    match (a.0, b.0) {
        (a, b) if a == b => Equal,
        (1 | 0, 0 | 1) => Equal,
        (1 | 0, _) => Greater,
        (_, 0 | 1) => Less,
        (_, _) => {
            let (a, _, _) = a.get(holder).unwrap();
            let (b, _, _) = b.get(holder).unwrap();
            a.cmp(&b)
        }
    }
}

impl<V: Hash + Ord + Eq + Clone> RawZdd<V> {
    ///Performs join (Minato, 1994 refers to this as "product") over two family
    ///subsets.
    ///
    ///It is defined as join(f, g) = { α ∪ β | α ∈ f, β ∈ g}
    ///
    ///# Panics
    ///May panic if `self` or `other` are undefined in the [`ZddHolder`].
    #[must_use]
    pub fn join(self, other: RawZdd<V>, holder: &mut ZddHolder<V>) -> RawZdd<V> {
        if cmp_tops(self, other, holder) == Greater {
            return other.join(self, holder);
        }

        if other.is_zero() {
            return RawZdd::ZERO;
        }
        if other.is_one() {
            return self;
        }

        let op = Operations::Join(self, other);
        if let Some(r) = holder.cache.get(&op) {
            return *r;
        }

        let (value, self_lo, self_hi) = self.get(holder).expect("Invalid index!");
        let (other_v, mut other_lo, mut other_hi) = other.get(holder).expect("Invalid index!");

        if other_v > value {
            other_lo = other;
            other_hi = RawZdd::ZERO;
        }
        let a = self_hi.join(other_hi, holder);
        let b = self_hi.join(other_lo, holder);
        let c = self_lo.join(other_hi, holder);
        let product = a.union(b, holder).union(c, holder);
        let v_product = holder.get_node_seq(Zdd {
            value,
            lo: RawZdd::ZERO,
            hi: product,
        });

        let joined = v_product.union(self_lo.join(other_lo, holder), holder);

        holder.cache.insert(op, joined);

        joined
    }

    ///Does `self` / {`v`} in the unate cube set algebra of Minato, 1994.
    ///
    ///It is defined as f / x = { α - x | α ∈ f ∧ x ∈ α}
    ///
    ///Identical to [`SetFamily::onset`]
    #[must_use]
    pub fn element_division(self, value: V, holder: &mut ZddHolder<V>) -> RawZdd<V> {
        self.onset(value, holder)
    }

    ///Does `self` % {`v`} in the unate cube set algebra of Minato, 1994.
    ///Identical to [`SetFamily::offset`]
    ///
    ///It is defined as f % x = { α | α ∉ f}
    #[must_use]
    pub fn element_remainder(self, value: V, holder: &mut ZddHolder<V>) -> RawZdd<V> {
        self.offset(value, holder)
    }

    /// The remainder of `self` divided by `other` according to the unate cub set algebra.
    ///
    /// For example, {abc,bc,ac}/{bc} = {a, {}}, so the remainder is {ac}. See [`SetFamily::divide`]
    /// for more details.
    ///
    ///# Panics
    ///May panic if `self` or `other` are undefined in the [`ZddHolder`] or **if `other` is
    ///[`SetFamily::ZERO`] (the empty set)**.
    #[must_use]
    pub fn remainder(self, other: RawZdd<V>, holder: &mut ZddHolder<V>) -> RawZdd<V> {
        self.difference(other.join(self.divide(other, holder), holder), holder)
    }

    ///Divides `self` by `other` according to the unate cube set algebra
    ///of Minato,
    ///
    /// This is defined by the quality:  f = g * (f/g) + (f%g) where * is [`SetFamily::join`]
    ///
    /// It can also be understood as: f / g = ⋂{ { α - β | α ∈ f ∧  β ⊆ α} | β ∈ g }
    ///
    /// For example, {abc,bc,ac}/{bc} = {a, {}} and {abd,abe,abg,cd,ce,ch}/{ab,c} = {d,e}
    ///
    ///# Panics
    ///May panic if `self` or `other` are undefined in the [`ZddHolder`] or **if `other` is
    ///[`SetFamily::ZERO`] (the empty set)**.
    #[must_use]
    pub fn divide(self, other: RawZdd<V>, holder: &mut ZddHolder<V>) -> RawZdd<V> {
        if other.is_one() {
            return self;
        }

        if self.is_zero() || self.is_one() {
            return RawZdd::ZERO;
        }
        if self == other {
            return RawZdd::ONE;
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
            r = r.intersect(r_l, holder);
        }

        holder.cache.insert(op, r);

        r
    }
}

#[cfg(test)]
mod test {
    use crate::RawZdd;

    use crate::algebra::test_op;

    #[test]
    fn test_join() {
        for (a, b, res) in [
            ("ab b c", "ab  ", "ab abc b c"),
            ("a b", "d c", "ad bd ac bc"),
            ("a b", "", ""),
            ("", "a b", ""),
            (" a", " b", " a b ab"),
            (" a", "b", "b ab"),
            ("a", " b", "a ab"),
            ("a", "b", "ab"),
            ("a", "a", "a"),
            ("a", "b c", "ab ac"),
            ("a b", "c", "ac bc"),
            ("a b c", "d", "ad bd cd"),
            ("a b", "  ", "a b"),
        ] {
            test_op(a, b, res, RawZdd::join, "*");
        }
    }

    #[test]
    fn test_divide() {
        for (a, b, res) in [
            ("a  ", "a", " "),
            ("abc bc ac", "bc", "a "),
            ("ab ac a", "a", "b c "),
            ("abd abe abg cd ce ch", "ab c", "d e"),
        ] {
            test_op(a, b, res, RawZdd::divide, "/");
        }
    }

    #[test]
    fn test_remainder() {
        for (a, b, res) in [
            ("a  ", "a", " "),
            ("abc bc ac", "bc", "ac"),
            ("ab ac a", "a", ""),
            ("abd abe abg cd ce ch", "ab c", "abg ch"),
        ] {
            test_op(a, b, res, RawZdd::remainder, "%");
        }
    }
}
