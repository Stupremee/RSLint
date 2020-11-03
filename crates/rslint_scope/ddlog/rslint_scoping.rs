use internment::Intern;
use once_cell::sync::Lazy;
use rslint_parser::{
    ast::{BinOp as AstBinOp, UnaryOp as AstUnaryOp},
    TextRange,
};
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
    GlobalId,
    FuncId,
    StmtId,
    ExprId,
}

/// The implicitly introduced `arguments` variable for function scopes,
/// kept in a global so we only allocate & intern it once
pub static IMPLICIT_ARGUMENTS: Lazy<Intern<Pattern>> = Lazy::new(|| {
    Intern::new(Pattern {
        name: Intern::new("arguments".to_owned()),
    })
});

impl From<AstUnaryOp> for UnaryOperand {
    fn from(op: AstUnaryOp) -> Self {
        match op {
            AstUnaryOp::Increment => UnaryOperand::UnaryIncrement,
            AstUnaryOp::Decrement => UnaryOperand::UnaryDecrement,
            AstUnaryOp::Delete => UnaryOperand::UnaryDelete,
            AstUnaryOp::Void => UnaryOperand::UnaryVoid,
            AstUnaryOp::Typeof => UnaryOperand::UnaryTypeof,
            AstUnaryOp::Plus => UnaryOperand::UnaryPlus,
            AstUnaryOp::Minus => UnaryOperand::UnaryMinus,
            AstUnaryOp::BitwiseNot => UnaryOperand::UnaryBitwiseNot,
            AstUnaryOp::LogicalNot => UnaryOperand::UnaryLogicalNot,
            AstUnaryOp::Await => UnaryOperand::UnaryAwait,
        }
    }
}

impl From<AstBinOp> for BinOperand {
    fn from(op: AstBinOp) -> Self {
        match op {
            AstBinOp::LessThan => BinOperand::BinLessThan,
            AstBinOp::GreaterThan => BinOperand::BinGreaterThan,
            AstBinOp::LessThanOrEqual => BinOperand::BinLessThanOrEqual,
            AstBinOp::GreaterThanOrEqual => BinOperand::BinGreaterThanOrEqual,
            AstBinOp::Equality => BinOperand::BinEquality,
            AstBinOp::StrictEquality => BinOperand::BinStrictEquality,
            AstBinOp::Inequality => BinOperand::BinInequality,
            AstBinOp::StrictInequality => BinOperand::BinStrictInequality,
            AstBinOp::Plus => BinOperand::BinPlus,
            AstBinOp::Minus => BinOperand::BinMinus,
            AstBinOp::Times => BinOperand::BinTimes,
            AstBinOp::Divide => BinOperand::BinDivide,
            AstBinOp::Remainder => BinOperand::BinRemainder,
            AstBinOp::Exponent => BinOperand::BinExponent,
            AstBinOp::LeftShift => BinOperand::BinLeftShift,
            AstBinOp::RightShift => BinOperand::BinRightShift,
            AstBinOp::UnsignedRightShift => BinOperand::BinUnsignedRightShift,
            AstBinOp::BitwiseAnd => BinOperand::BinBitwiseAnd,
            AstBinOp::BitwiseOr => BinOperand::BinBitwiseOr,
            AstBinOp::BitwiseXor => BinOperand::BinBitwiseXor,
            AstBinOp::NullishCoalescing => BinOperand::BinNullishCoalescing,
            AstBinOp::LogicalOr => BinOperand::BinLogicalOr,
            AstBinOp::LogicalAnd => BinOperand::BinLogicalAnd,
            AstBinOp::In => BinOperand::BinIn,
            AstBinOp::Instanceof => BinOperand::BinInstanceof,
        }
    }
}
