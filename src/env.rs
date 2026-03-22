//! The runtime environment for symbol bindings.
//!
//! Uses Arc<Env> for efficient environment sharing. When entering a function,
//! the captured environment is stored as an Arc, allowing O(1) environment
//! saves/restores via Arc cloning instead of deep-cloning HashMaps.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use std::thread_local;

thread_local! {
    pub static REPL_ENV: RefCell<Arc<Env>> = RefCell::new(Arc::new(Env::root()));
}

pub fn get_repl_env() -> Arc<Env> {
    REPL_ENV.with(|env| env.borrow().clone())
}

pub fn set_repl_env(env: Arc<Env>) {
    REPL_ENV.with(|re| *re.borrow_mut() = env);
}

pub fn repl_insert(name: String, value: SAtom) {
    REPL_ENV.with(|re| {
        let mut repl = re.borrow_mut();
        let new_env = Arc::make_mut(&mut repl);
        new_env.local.insert(name, value);
    });
}

pub fn repl_lookup(name: &str) -> Option<SAtom> {
    REPL_ENV.with(|env| env.borrow().lookup(name))
}

pub fn repl_define(name: String, value: SAtom) {
    REPL_ENV.with(|re| {
        let mut repl = re.borrow_mut();
        Arc::make_mut(&mut repl).local.insert(name, value);
    });
}

use crate::atom::{Atom, Fun, SAtom};
use crate::lisp_eval::apply_callable_step;
use crate::nil;
use crate::sexpr::SExpr;
use crate::t;
use crate::types::{Args, EvalResult, Step};

/// The runtime environment.
///
/// Uses a parent chain with copy-on-write semantics via Arc.
/// Lookup walks up the parent chain, but insertions only modify
/// the local scope (via Arc::make_mut for COW).
#[derive(Clone, Debug, PartialEq)]
pub struct Env {
    pub(crate) parent: Option<Arc<Env>>,
    pub(crate) local: HashMap<String, SAtom>,
}

impl Env {
    /// Creates a new root environment with builtins.
    pub fn root() -> Self {
        let mut env = Self::default();
        env.builtins_init();
        env
    }

    /// Creates a new child environment with the given parent.
    #[inline]
    pub fn with_parent(parent: Arc<Env>) -> Self {
        Self {
            parent: Some(parent),
            local: HashMap::new(),
        }
    }

    /// Looks up a name in the environment chain.
    #[inline]
    pub fn lookup(&self, name: &str) -> Option<SAtom> {
        self.local
            .get(name)
            .cloned()
            .or_else(|| self.parent.as_ref()?.lookup(name))
    }

    #[inline]
    pub fn insert(env: &mut Arc<Env>, name: String, value: SAtom) {
        Arc::make_mut(env).local.insert(name, value);
    }

    /// Gets a mutable reference to local bindings.
    /// Uses Arc::make_mut for copy-on-write semantics.
    #[inline]
    pub fn local_mut(env: &mut Arc<Env>) -> &mut HashMap<String, SAtom> {
        &mut Arc::make_mut(env).local
    }

    fn builtins_init(&mut self) {
        macro_rules! take_args {
            ($it:expr; $($name:ident),+ $(,)?) => {{
                (|| -> Option<_> {
                    let mut iter = ($it).iter();
                    $( let $name: SAtom = iter.next()?; )+
                    Some(($($name),+))
                })()
            }};
        }

        #[inline]
        fn get_args_count(args: &Args) -> usize {
            match args {
                Args::S(sexpr) => sexpr.len,
                Args::Nil => 0,
            }
        }

        #[inline]
        fn expect_exact_args(args: &Args, n: usize, msg: &'static str) -> Result<(), &'static str> {
            if get_args_count(args) == n {
                Ok(())
            } else {
                Err(msg)
            }
        }

        #[inline]
        fn expect_min_args(args: &Args, n: usize, msg: &'static str) -> Result<(), &'static str> {
            if get_args_count(args) >= n {
                Ok(())
            } else {
                Err(msg)
            }
        }

        #[inline]
        fn get_num_value(v: &Atom) -> Result<f64, &'static str> {
            match v {
                Atom::Num(n) => Ok(*n),
                _ => Err("expected number"),
            }
        }

        #[inline]
        fn native_value<F>(f: F) -> Fun
        where
            F: Fn(&mut Arc<Env>, &Args) -> EvalResult + Send + Sync + 'static,
        {
            Fun::Native(Box::new(move |env, args, _stack| {
                Ok(Step::Value(f(env, args)?))
            }))
        }

        #[inline]
        fn fun_atom(fun: Fun) -> SAtom {
            Arc::new(Atom::Fun(Arc::new(fun)))
        }

        let binary_ops = |op: fn(f64, f64) -> f64| {
            native_value(move |_: &mut Arc<Env>, args: &Args| {
                expect_min_args(args, 2, "expected at least 2 args")?;
                match args {
                    Args::S(args) => {
                        let mut iter = args.iter();
                        let first = iter.next().ok_or("expected at least 2 args")?;
                        let mut acc = get_num_value(first.as_ref())?;
                        for v in iter {
                            acc = op(acc, get_num_value(v.as_ref())?);
                        }
                        Ok(crate::num!(acc).into())
                    }
                    Args::Nil => Err("expected at least 2 args"),
                }
            })
        };

        let car_op = native_value(|_: &mut Arc<Env>, args: &Args| {
            expect_exact_args(args, 1, "car expects exactly 1 arg")?;
            match args {
                Args::S(sexpr) => match sexpr.car.as_ref() {
                    Atom::Nil => Ok(nil!().into()),
                    Atom::Cons(list) => Ok(list.car.clone()),
                    _ => Err("car expects a list"),
                },
                Args::Nil => unreachable!(),
            }
        });

        let cdr_op = native_value(|_: &mut Arc<Env>, args: &Args| {
            expect_exact_args(args, 1, "cdr expects exactly 1 arg")?;
            match args {
                Args::S(sexpr) => match sexpr.car.as_ref() {
                    Atom::Nil => Ok(nil!().into()),
                    Atom::Cons(list) => Ok(list.cdr.clone()),
                    _ => Err("cdr expects a list"),
                },
                Args::Nil => unreachable!(),
            }
        });

        let apply_op = Fun::Native(Box::new(|s: &mut Arc<Env>, args: &Args, stack| {
            let (callable, rest) = match args {
                Args::S(sexpr) => (sexpr.car.clone(), sexpr.cdr.clone()),
                Args::Nil => return Err("apply expects at least a function and one list arg"),
            };
            let flat_args_atom = match rest.as_ref() {
                Atom::Cons(rest_list) => {
                    let mut items: Vec<SAtom> = rest_list.iter().collect();
                    let last = items
                        .pop()
                        .ok_or("apply expects at least one trailing argument list")?;
                    match last.as_ref() {
                        Atom::Nil => {}
                        Atom::Cons(inner) => items.extend(inner.iter()),
                        _ => return Err("last argument to apply must be a list"),
                    }
                    SExpr::from_satoms(items)
                }
                Atom::Nil => return Err("apply expects at least a function and one list arg"),
                _ => return Err("apply received invalid argument list"),
            };
            match apply_callable_step(callable, flat_args_atom, s, stack) {
                Err("Only callable values can be used for calling") => {
                    Err("first element is not callable")
                }
                other => other,
            }
        }));

        let funcall_op = Fun::Native(Box::new(|s: &mut Arc<Env>, args: &Args, stack| {
            let (callable, rest) = match args {
                Args::S(sexpr) => (sexpr.car.clone(), sexpr.cdr.clone()),
                Args::Nil => return Err("funcall expects at least a function"),
            };
            match apply_callable_step(callable, rest, s, stack) {
                Err("Only callable values can be used for calling") => {
                    Err("first element is not callable")
                }
                other => other,
            }
        }));

        let list_op = native_value(|_: &mut Arc<Env>, args: &Args| -> EvalResult {
            match args {
                Args::S(sexpr) => Ok(Arc::new(Atom::Cons((**sexpr).clone()))),
                Args::Nil => Ok(SExpr::empty_list()),
            }
        });

        let cons_op = native_value(|_: &mut Arc<Env>, args: &Args| -> EvalResult {
            expect_exact_args(args, 2, "cons expects exactly 2 args")?;
            match args {
                Args::S(args) => {
                    let (car, cdr) =
                        take_args!(args; car, cdr).ok_or("cons expects exactly 2 args")?;
                    let len = 1 + match cdr.as_ref() {
                        Atom::Nil => 0,
                        Atom::Cons(sexpr) => sexpr.len,
                        _ => 1,
                    };
                    Ok(Arc::new(Atom::Cons(SExpr { car, cdr, len })))
                }
                Args::Nil => unreachable!(),
            }
        });

        let eq_op = native_value(|_: &mut Arc<Env>, args: &Args| -> EvalResult {
            expect_exact_args(args, 2, "eq expects exactly 2 args")?;
            match args {
                Args::S(sexpr) => {
                    let (x, y) = take_args!(sexpr; x, y).ok_or("eq expects exactly 2 args")?;
                    if x.as_ref() == y.as_ref() {
                        Ok(t!().into())
                    } else {
                        Ok(nil!().into())
                    }
                }
                Args::Nil => unreachable!(),
            }
        });

        self.local.insert("nil".into(), nil!().into());
        self.local.insert("t".into(), t!().into());
        self.local
            .insert("add".into(), fun_atom(binary_ops(|a, b| a + b)));
        self.local
            .insert("mul".into(), fun_atom(binary_ops(|a, b| a * b)));
        self.local
            .insert("sub".into(), fun_atom(binary_ops(|a, b| a - b)));
        self.local
            .insert("div".into(), fun_atom(binary_ops(|a, b| a / b)));
        self.local.insert("car".into(), fun_atom(car_op));
        self.local.insert("cdr".into(), fun_atom(cdr_op));
        self.local.insert("list".into(), fun_atom(list_op));
        self.local.insert("apply".into(), fun_atom(apply_op));
        self.local.insert("funcall".into(), fun_atom(funcall_op));
        self.local.insert("cons".into(), fun_atom(cons_op));
        self.local.insert("eq".into(), fun_atom(eq_op));
    }
}

impl Default for Env {
    fn default() -> Self {
        Self {
            parent: None,
            local: HashMap::new(),
        }
    }
}
