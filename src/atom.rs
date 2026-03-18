use crate::{
    env::Env,
    lisp_eval::{Args, EvalResult},
    sexpr::{SExpr, SExprIter, Cursor},
};
use std::{collections::HashMap, fmt::Debug, ptr, sync::Arc};

pub type NativeFn = Box<dyn Fn(&mut Env, &Args) -> EvalResult + Send + Sync>;
#[derive(Clone, PartialEq)]
pub struct UserFn {
    pub params: Vec<String>,
    pub body: SAtom,
    pub captured_val: HashMap<String, SAtom>,
}

pub type SAtom = Arc<Atom>;

pub enum Fun {
    Native(NativeFn),
    User(UserFn),
}

impl Fun {
    pub fn call(&self, env: &mut Env, args: &Args) -> EvalResult {
        match self {
            Fun::Native(s_fun) => s_fun(env, args),
            Fun::User(_) => Err("internal error: user functions must be applied by evaluator"),
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

impl Atom {
    pub fn list_get(&self, index: usize) -> Option<&Atom> {
        match self {
            Atom::Nil => None,
            Atom::Cons(sexpr) => sexpr.get(index),
            _ => None,
        }
    }

    pub fn list_iter(&self) -> SExprIter<'_> {
        match self {
            Atom::Nil => SExprIter {
                cursor: Cursor::Done,
            },
            Atom::Cons(sexpr) => sexpr.iter(),
            _ => panic!("not a list"),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct List(pub SAtom);

impl<T> FromIterator<T> for List
where
    T: Into<SAtom>,
{
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        List(SExpr::from_satoms(iter.into_iter().map(Into::into)))
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
