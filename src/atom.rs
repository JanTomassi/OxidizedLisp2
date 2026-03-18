//! Core data types for the Lisp interpreter.
//!
//! This module defines the fundamental data structures:
//! - [`Atom`]: The enum of all value types in the language
//! - [`SAtom`]: A shared, thread-safe reference to an Atom (via Arc)
//! - [`Fun`]: Function values (native or user-defined)
//! - [`UserFn`]: A user-defined lambda with captured environment

use std::{collections::HashMap, fmt::Debug, ptr, sync::Arc};

use crate::env::EvaluatorEnv;
use crate::sexpr::SExpr;
use crate::types::{Args, Frame, Step};

/// A user-defined function (lambda).
///
/// User functions capture the lexical environment at definition time,
/// forming closures. The captured environment is stored in `captured_val`
/// and restored when the function returns.
#[derive(Clone, PartialEq)]
pub struct UserFn {
    pub params: Vec<String>,
    pub body: SAtom,
    pub captured_val: HashMap<String, SAtom>,
}

/// Shared reference to an Atom.
///
/// Using Arc allows multiple parts of the evaluator to share values
/// without cloning, and enables efficient constant values like nil and t.
pub type SAtom = Arc<Atom>;

/// Function values.
///
/// Functions can be either:
/// - Native: implemented in Rust for performance or I/O
/// - User: defined in Lisp as lambdas, capturing lexical scope
pub enum Fun {
    Native(
        Box<dyn Fn(&mut dyn EvaluatorEnv, &Args, &mut Vec<Frame>) -> Result<Step, &'static str>>,
    ),
    User(UserFn),
}

impl Fun {
    /// Calls a native function, returning a Step.
    ///
    /// User functions must be applied by the evaluator's trampoline loop,
    /// so this method only handles native functions.
    pub fn call(
        &self,
        env: &mut dyn EvaluatorEnv,
        args: &Args,
        stack: &mut Vec<Frame>,
    ) -> Result<Step, &'static str> {
        match self {
            Fun::Native(f) => f(env, args, stack),
            Fun::User(_) => Err("user functions must be applied by evaluator"),
        }
    }
}

/// The fundamental value type in the Lisp runtime.
///
/// Atoms can be:
/// - `T` / `Nil`: boolean-like constants
/// - `Num`: floating-point numbers
/// - `Str`: strings
/// - `Sym`: symbols (identifiers)
/// - `Cons`: cons cells (pairs/lists)
/// - `Fun`: function values (native or user)
#[derive(Clone)]
pub enum Atom {
    T,
    Nil,
    Num(f64),
    Str(String),
    Sym(String),
    Cons(SExpr),
    Fun(Arc<Fun>),
}

impl Atom {
    /// Gets an element from a list by index.
    ///
    /// Returns `None` if the index is out of bounds or if this atom
    /// is not a proper list.
    pub fn list_get(&self, index: usize) -> Option<&Atom> {
        match self {
            Atom::Nil => None,
            Atom::Cons(sexpr) => sexpr.get(index),
            _ => None,
        }
    }

    /// Returns an iterator over the elements of a list.
    ///
    /// # Panics
    ///
    /// Panics if this atom is not a list (Cons or Nil).
    pub fn list_iter(&self) -> crate::sexpr::SExprIter<'_> {
        match self {
            Atom::Nil => crate::sexpr::SExprIter {
                cursor: crate::sexpr::Cursor::Done,
            },
            Atom::Cons(sexpr) => sexpr.iter(),
            _ => panic!("not a list"),
        }
    }
}

/// A list wrapper for convenience iteration.
#[derive(Clone, PartialEq, Debug)]
pub struct List(pub SAtom);

impl<T> FromIterator<T> for List
where
    T: Into<SAtom>,
{
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        List(crate::sexpr::SExpr::from_satoms(
            iter.into_iter().map(Into::into),
        ))
    }
}

impl PartialEq for Atom {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Atom::T, Atom::T) | (Atom::Nil, Atom::Nil) => true,
            (Atom::Num(a), Atom::Num(b)) => a == b,
            (Atom::Str(a), Atom::Str(b)) => a == b,
            (Atom::Sym(a), Atom::Sym(b)) => a == b,
            (Atom::Cons(a), Atom::Cons(b)) => a == b,

            (Atom::Fun(a), Atom::Fun(b)) => match (&**a, &**b) {
                (Fun::Native(a), Fun::Native(b)) => ptr::eq(&**a, &**b),
                (Fun::User(a), Fun::User(b)) => a == b,
                _ => false,
            },
            _ => false,
        }
    }
}

impl From<f64> for Atom {
    fn from(v: f64) -> Self {
        Atom::Num(v)
    }
}

impl From<SExpr> for Atom {
    fn from(v: SExpr) -> Self {
        Atom::Cons(v)
    }
}

impl Debug for Atom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Atom::Num(n) => write!(f, "{}", n),
            Atom::Str(s) => write!(f, "{:?}", s),
            Atom::Sym(s) => write!(f, "{}", s),
            Atom::Nil => write!(f, "Nil"),
            Atom::T => write!(f, "T"),
            Atom::Cons(sexpr) => sexpr.fmt(f),
            Atom::Fun(fun) => match &**fun {
                Fun::Native(_) => write!(f, "NativeFn"),
                Fun::User(fun) => write!(f, "[{:#?} | {:#?}]", fun.params, fun.body),
            },
        }
    }
}

impl Default for Atom {
    fn default() -> Self {
        Atom::Nil
    }
}
