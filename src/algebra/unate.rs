use crate::{Operations, SetFamily};
use std::{
    cmp::Ordering::{Equal, Greater, Less},
    hash::Hash,
};

pub(crate) fn cmp_tops<V: Ord + Hash + Eq + Clone>(
    a: &SetFamily<V>,
    b: &SetFamily<V>,
) -> std::cmp::Ordering {
    match (a.id, b.id) {
        (a, b) if a == b => Equal,
        (1 | 0, 0 | 1) => Equal,
        (1 | 0, _) => Greater,
        (_, 0 | 1) => Less,
        (_, _) => {
            let (a, _, _) = a.get().unwrap();
            let (b, _, _) = b.get().unwrap();
            a.cmp(&b)
        }
    }
}

impl<'a, V: Hash + Ord + Eq + Clone + Send + Sync> SetFamily<'a, V> {
    ///Does `self` % {`v`} in the unate cube set algebra of Minato, 1994.
    ///Identical to [`SetFamily::offset`]
    ///
    ///It is defined as f % x = { α | α ∉ f}
    #[must_use]
    pub fn element_remainder(self, value: V) -> SetFamily<'a, V> {
        self.offset(value)
    }

    ///Does `self` / {`v`} in the unate cube set algebra of Minato, 1994.
    ///
    ///It is defined as f / x = { α - x | α ∈ f ∧ x ∈ α}
    ///
    ///Identical to [`SetFamily::onset`]
    #[must_use]
    pub fn element_division(self, value: V) -> SetFamily<'a, V> {
        self.onset(value)
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
    #[expect(clippy::needless_pass_by_value)]
    pub fn divide(self, other: SetFamily<'a, V>) -> SetFamily<'a, V> {
        if other.is_one() {
            return self.clone();
        }

        let holder = self.manager;

        if self.is_zero() || self.is_one() {
            return holder.zero();
        }
        if self == other {
            return holder.one();
        }

        let (value, other_lo, other_hi) = other.get().expect("Can't divide by the empty set!");

        if other_lo.is_zero() && other_hi.is_one() {
            return self.element_division(value.clone());
        }

        let op = Operations::Division(self.as_raw(), other.as_raw());
        if let Some(r) = holder.get_from_cache(&op) {
            return r;
        }

        let value = value.clone();
        let r_lo = self.clone().element_division(value.clone());

        let mut r = r_lo.divide(other_hi);

        if !r.is_zero() && !other_lo.is_zero() {
            let r_h = self.element_remainder(value);
            let r_l = r_h.divide(other_lo);
            r = r.intersect(r_l);
        }

        holder.put_into_cache(op, r)
    }

    ///Performs join (Minato, 1994 refers to this as "product") over two family
    ///subsets.
    ///
    ///It is defined as join(f, g) = { α ∪ β | α ∈ f, β ∈ g}
    ///
    ///# Panics
    ///May panic if `self` or `other` are undefined in the [`ZddHolder`].
    #[must_use]
    pub fn join(self, other: SetFamily<'a, V>) -> SetFamily<'a, V> {
        if cmp_tops(&self, &other) == Greater {
            return other.join(self);
        }

        if other.is_zero() {
            return other;
        }

        if other.is_one() {
            return self;
        }

        let holder = self.manager;
        let op = Operations::Join(self.as_raw(), other.as_raw());
        if let Some(r) = holder.get_from_cache(&op) {
            return r;
        }

        let (value, self_lo, self_hi) = self.get().expect("Invalid index!");
        let (other_v, mut other_lo, mut other_hi) = other.get().expect("Invalid index!");

        if other_v > value {
            other_lo = other;
            other_hi = self.manager.zero();
        }

        let self_hi_clone = self_hi.clone();
        let other_lo_clone = other_lo.clone();
        let other_hi_clone = other_hi.clone();
        let (a, (b, c)) = self.manager.pools().join(
            || self_hi_clone.join(other_hi),
            || {
                self.manager.pools().join(
                    || self_hi.join(other_lo_clone),
                    || self_lo.clone().join(other_hi_clone),
                )
            },
        );

        let product = a.union(b).union(c);
        let v_product = holder.get_node(value, holder.zero(), product);

        let joined = v_product.union(self_lo.join(other_lo));

        holder.put_into_cache(op, joined)
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
    pub fn remainder(self, other: Self) -> Self {
        self.clone()
            .difference(other.clone().join(self.divide(other)))
    }

    /// The minimal of elements of `self`, e.g.
    ///
    /// `f.minimal()` = {x ∈ f | y ∈ f and x ⊇ y implies x=y }
    ///
    ///# Panics
    ///May panic if `self` or `other` are undefined in the [`ZddHolder`].
    #[must_use]
    pub fn minimal_elements(self) -> Self {
        if self.is_zero() || self.is_one() {
            return self;
        }

        let op = Operations::Minimal(self.as_raw());
        if let Some(r) = self.manager().get_from_cache(&op) {
            return r;
        }

        let (v, lo, hi) = self.get().unwrap();
        let r_l = lo.minimal_elements();
        let r = hi.minimal_elements();
        let r_h = r.nonsup(r_l.clone());

        let r = self.manager().get_node(v, r_l, r_h);

        self.manager().put_into_cache(op, r)
    }

    ///The non-superset of `self` and `other`.
    ///
    /// f.nonsup(g) = {x ∈ f | y ∈ g implies x ⊉ y }
    ///
    ///# Panics
    ///May panic if `self` or `other` are undefined in the [`ZddHolder`].
    #[must_use]
    pub fn nonsup(self, other: Self) -> Self {
        if other.is_zero() {
            return self;
        }

        if self.is_zero() || other.is_one() || self == other {
            return self.manager.zero();
        }

        let op = Operations::NonSup(self.as_raw(), other.as_raw());
        if let Some(r) = self.manager().get_from_cache(&op) {
            return r;
        }

        let (o_val, o_lo, o_hi) = other.get().unwrap();

        if self.is_one() {
            //If self is one, then the lhs must not contain the empty set.
            let mut o_lo = o_lo;
            while let Some((_, new_lo, _)) = o_lo.get() {
                o_lo = new_lo;
            }
            return if o_lo.is_zero() {
                self
            } else {
                self.manager.zero()
            };
        }

        let (s_val, s_lo, s_hi) = self.get().unwrap();

        if s_val > o_val {
            return self.nonsup(o_lo);
        }
        let v = s_val;
        let r = if v < o_val {
            let r_l = s_lo.nonsup(other.clone());
            let r_h = s_hi.nonsup(other);
            self.manager().get_node(v, r_l, r_h)
        } else {
            let r_l = s_hi.clone().nonsup(o_hi);
            let r = s_hi.nonsup(o_lo.clone());
            let r_h = r.intersect(r_l);
            let r_l = s_lo.nonsup(o_lo);

            self.manager().get_node(v, r_l, r_h)
        };
        self.manager().put_into_cache(op, r.clone())
    }
}

#[cfg(test)]
mod test {
    #![expect(clippy::redundant_closure_for_method_calls)]
    use crate::{
        ZddHolder,
        algebra::{test_op, test_solo_op},
    };

    #[test]
    fn test_join() {
        let holder = ZddHolder::new();
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
            ("", "", ""),
            (" ", "", ""),
            ("", " ", ""),
            (" ", " ", " "),
        ] {
            test_op(a, b, res, |x, y| x.join(y), "*", &holder);
        }
    }
    #[test]
    fn test_nonsup() {
        let holder = ZddHolder::new();
        for (a, b, res) in [
            (" ", " a", ""),
            (" ", "a", " "),
            ("a b c", "d", "a b c"),
            ("a b", "c", "a b"),
            ("a b cd", "d", "a b"),
            ("a b cdwe ", "c", "a b "),
        ] {
            test_op(a, b, res, |x, y| x.nonsup(y), "↘", &holder);
        }
    }

    #[test]
    fn test_minimal() {
        let holder = ZddHolder::new();
        for (a, res) in [
            ("", ""),
            ("a", "a"),
            ("a ", " "),
            ("a b c", "a b c"),
            ("ab b bc ca", "b ca"),
        ] {
            test_solo_op(a, res, |x| x.minimal_elements(), "↓", &holder);
        }
    }

    #[test]
    fn test_divide() {
        let holder = ZddHolder::new();
        for (a, b, res) in [
            ("a  ", "a", " "),
            ("abc bc ac", "bc", "a "),
            ("ab ac a", "a", "b c "),
            ("abd abe abg cd ce ch", "ab c", "d e"),
        ] {
            test_op(a, b, res, |x, y| x.divide(y), "/", &holder);
        }
    }

    #[test]
    fn test_remainder() {
        let holder = ZddHolder::new();
        for (a, b, res) in [
            ("a  ", "a", " "),
            ("abc bc ac", "bc", "ac"),
            ("ab ac a", "a", ""),
            ("abd abe abg cd ce ch", "ab c", "abg ch"),
        ] {
            test_op(a, b, res, |x, y| x.remainder(y), "%", &holder);
        }
    }
}
