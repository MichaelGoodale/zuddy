use std::{
    fmt::{Debug, Display},
    hash::Hash,
    ops::{Add, AddAssign},
};

use thiserror::Error;

///Represents a usize, or positive infinity
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub enum UsizeOrPositiveInfinity {
    ///A usize
    Size(usize),
    ///Positive Infinity
    PositiveInfinity,
}
impl Display for UsizeOrPositiveInfinity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsizeOrPositiveInfinity::Size(x) => write!(f, "{x}"),
            UsizeOrPositiveInfinity::PositiveInfinity => write!(f, "∞"),
        }
    }
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

impl AddAssign for UsizeOrPositiveInfinity {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
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

///Isize or positive or negative infinity
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum IsizeOrInfinity {
    ///Negative Infinity
    NegInfinity,
    ///Any finite value
    Finite(isize),
    ///Positive Infinity
    PosInfinity,
}

impl AddAssign for IsizeOrInfinity {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Add for IsizeOrInfinity {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (IsizeOrInfinity::Finite(x), IsizeOrInfinity::Finite(y)) => {
                IsizeOrInfinity::Finite(x + y)
            }
            (
                IsizeOrInfinity::NegInfinity | IsizeOrInfinity::Finite(_),
                IsizeOrInfinity::NegInfinity,
            )
            | (IsizeOrInfinity::NegInfinity, IsizeOrInfinity::Finite(_)) => {
                IsizeOrInfinity::NegInfinity
            }
            (
                IsizeOrInfinity::PosInfinity | IsizeOrInfinity::Finite(_),
                IsizeOrInfinity::PosInfinity,
            )
            | (IsizeOrInfinity::PosInfinity, IsizeOrInfinity::Finite(_)) => {
                IsizeOrInfinity::PosInfinity
            }
            (IsizeOrInfinity::PosInfinity, IsizeOrInfinity::NegInfinity)
            | (IsizeOrInfinity::NegInfinity, IsizeOrInfinity::PosInfinity) => {
                panic!("Negative infinity plus positive infinity is undefined!")
            }
        }
    }
}

impl Display for IsizeOrInfinity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IsizeOrInfinity::NegInfinity => write!(f, "-∞"),
            IsizeOrInfinity::Finite(x) => write!(f, "{x}"),
            IsizeOrInfinity::PosInfinity => write!(f, "∞"),
        }
    }
}

pub trait PossiblyInfinite: Copy + PartialEq + TryFrom<Self::PossiblyInfiniteType> {
    type PossiblyInfiniteType: Copy
        + Clone
        + PartialEq
        + Ord
        + AddAssign
        + Add<Output = Self::PossiblyInfiniteType>;

    fn as_finite(self) -> Self::PossiblyInfiniteType;
}

#[derive(Debug, Error)]
#[error("This value is not finite!")]
pub struct IsInfinite;

impl TryFrom<IsizeOrInfinity> for isize {
    type Error = IsInfinite;

    fn try_from(value: IsizeOrInfinity) -> Result<Self, Self::Error> {
        match value {
            IsizeOrInfinity::NegInfinity | IsizeOrInfinity::PosInfinity => Err(IsInfinite),
            IsizeOrInfinity::Finite(x) => Ok(x),
        }
    }
}

impl PossiblyInfinite for isize {
    type PossiblyInfiniteType = IsizeOrInfinity;

    fn as_finite(self) -> Self::PossiblyInfiniteType {
        IsizeOrInfinity::Finite(self)
    }
}

impl TryFrom<UsizeOrPositiveInfinity> for usize {
    type Error = IsInfinite;

    fn try_from(value: UsizeOrPositiveInfinity) -> Result<Self, Self::Error> {
        match value {
            UsizeOrPositiveInfinity::PositiveInfinity => Err(IsInfinite),
            UsizeOrPositiveInfinity::Size(x) => Ok(x),
        }
    }
}

impl PossiblyInfinite for usize {
    type PossiblyInfiniteType = UsizeOrPositiveInfinity;

    fn as_finite(self) -> Self::PossiblyInfiniteType {
        UsizeOrPositiveInfinity::Size(self)
    }
}
