use rslint_parser::TextRange;
use std::{
    cell::Cell,
    ops::{Add, AddAssign, Range},
};

/// Allow emitting debug messages from within datalog
// TODO: Replace with tracing
pub fn debug(message: &String) {
    println!("[datalog debug]: {}", message);
}

/// Implement the `Span` trait for ddlog `Span`s
impl rslint_errors::Span for Span {
    fn as_range(&self) -> Range<usize> {
        self.start as usize..self.end as usize
    }
}

/// Allow converting a `TextRange` into a ddlog `Span`
impl From<TextRange> for Span {
    fn from(range: TextRange) -> Self {
        Self {
            start: range.start().into(),
            end: range.end().into(),
        }
    }
}

macro_rules! impl_id_traits {
    ($($ty:ty),* $(,)?) => {
        /// A convenience trait to allow easily incrementing ids during ast->ddlog translation
        pub trait Increment {
            type Inner;

            /// Increments the id by one, returning the value *before* it was incremented
            fn inc(&self) -> Self::Inner;
        }

        $(
            impl $ty {
                /// Creates a new id from the given value
                pub const fn new(id: u32) -> Self {
                    Self { id }
                }
            }

            impl Increment for Cell<$ty> {
                type Inner = $ty;

                fn inc(&self) -> Self::Inner {
                    let old = self.get();
                    self.set(old + 1);
                    old
                }
            }

            impl From<u32> for $ty {
                fn from(id: u32) -> Self {
                    Self { id }
                }
            }

            impl Add for $ty {
                type Output = Self;

                fn add(self, other: Self) -> Self {
                    Self {
                        id: self.id + other.id,
                    }
                }
            }

            impl Add<u32> for $ty {
                type Output = Self;

                fn add(self, other: u32) -> Self {
                    Self {
                        id: self.id + other,
                    }
                }
            }

            impl AddAssign for $ty {
                fn add_assign(&mut self, other: Self) {
                    self.id += other.id;
                }
            }

            impl AddAssign<u32> for $ty {
                fn add_assign(&mut self, other: u32) {
                    self.id += other;
                }
            }

            // They're all small types and so can be trivially copied
            impl Copy for $ty {}
        )*
    };
}

// Implement basic traits for id type-safe wrappers
impl_id_traits! {
    Scope,
    FuncId,
    StmtId,
    ExprId,
}
