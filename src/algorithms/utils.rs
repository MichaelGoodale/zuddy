use std::ops::Add;

///Represents a usize, or positive infinity
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub enum UsizeOrPositiveInfinity {
    ///A usize
    Size(usize),
    ///Positive Infinity
    PositiveInfinity,
}

impl From<UsizeOrPositiveInfinity> for Option<usize> {
    fn from(value: UsizeOrPositiveInfinity) -> Self {
        match value {
            UsizeOrPositiveInfinity::Size(x) => Some(x),
            UsizeOrPositiveInfinity::PositiveInfinity => None,
        }
    }
}

impl Add for UsizeOrPositiveInfinity {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (UsizeOrPositiveInfinity::Size(x), UsizeOrPositiveInfinity::Size(y)) => x
                .checked_add(y)
                .map_or(UsizeOrPositiveInfinity::PositiveInfinity, |z| {
                    UsizeOrPositiveInfinity::Size(z)
                }),
            _ => UsizeOrPositiveInfinity::PositiveInfinity,
        }
    }
}

impl UsizeOrPositiveInfinity {
    ///Adds a value to a [`UsizeOrPositiveInfinity`], turning to [`UsizeOrPositiveInfinity::PositiveInfinity`] if there is an
    ///overflow.
    #[must_use]
    pub fn add_usize(self, x: usize) -> Self {
        match self {
            UsizeOrPositiveInfinity::Size(s) => s
                .checked_add(x)
                .map_or(UsizeOrPositiveInfinity::PositiveInfinity, |z| {
                    UsizeOrPositiveInfinity::Size(z)
                }),
            UsizeOrPositiveInfinity::PositiveInfinity => UsizeOrPositiveInfinity::PositiveInfinity,
        }
    }

    ///Take a [`UsizeOrPositiveInfinity`] and unwrap it, assuming it is
    ///[`UsizeOrPositiveInfinity::Size`]
    ///
    ///# Panics
    ///Will panic if this is [`UsizeOrPositiveInfinity::PositiveInfinity`]
    #[must_use]
    pub fn unwrap(self) -> usize {
        match self {
            UsizeOrPositiveInfinity::Size(x) => x,
            UsizeOrPositiveInfinity::PositiveInfinity => panic!("Size is infinite!"),
        }
    }
}
