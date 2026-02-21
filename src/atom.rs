use crate::{
    env::Env,
    lisp_eval::{Args, EvalResult},
    sexpr::SExpr,
};
use std::{fmt::Debug, ptr, sync::Arc};

pub type NativeFn = Box<dyn Fn(&mut Env, &Args) -> EvalResult + Send + Sync>;
pub type UserFn = Box<(
    SAtom,
    Box<dyn Fn(&mut Env, &Args) -> EvalResult + Send + Sync>,
)>;
pub type SAtom = Arc<Atom>;

pub enum Fun {
    Native(NativeFn),
    User(UserFn),
}

impl Fun {
    pub fn call(&self, env: &mut Env, args: &Args) -> EvalResult {
        match self {
            Fun::Native(s_fun) => s_fun(env, args),
            Fun::User(s_fun) => s_fun.1(env, args),
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
                (Fun::User(a), Fun::User(b)) => a.0 == b.0,
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
                Fun::User(fun) => write!(f, "{:#?}", fun.0),
            },
        }
    }
}

impl Default for Atom {
    fn default() -> Self {
        Atom::Nil
    }
}
