//! Core data types for the Lisp interpreter.

use std::{fmt::Debug, ptr, sync::Arc};

use crate::env::Env;
use crate::sexpr::SExpr;
use crate::types::{Args, Frame, Step};

/// A user-defined function (lambda).
///
/// Captures the lexical environment as an Arc for efficient closure semantics.
#[derive(Clone, PartialEq, Debug)]
pub struct UserFn {
    pub params: Vec<String>,
    pub body: SAtom,
    pub captured_env: Arc<Env>,
}

pub type SAtom = Arc<Atom>;

pub enum Fun {
    Native(
        Box<
            dyn Fn(&mut Arc<Env>, &Args, &mut Vec<Frame>) -> Result<Step, &'static str>
                + Send
                + Sync,
        >,
    ),
    User(UserFn),
}

impl Fun {
    pub fn call(
        &self,
        env: &mut Arc<Env>,
        args: &Args,
        stack: &mut Vec<Frame>,
    ) -> Result<Step, &'static str> {
        match self {
            Fun::Native(f) => f(env, args, stack),
            Fun::User(_) => Err("user functions must be applied by evaluator"),
        }
    }
}

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
