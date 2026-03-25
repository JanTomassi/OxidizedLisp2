//! The runtime environment for symbol bindings.
//!
//! Uses Arc<Env> for efficient environment sharing. When entering a function,
//! the captured environment is stored as an Arc, allowing O(1) environment
//! saves/restores via Arc cloning instead of deep-cloning HashMaps.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use crate::atom::{Atom, Fun, SAtom};
use crate::lisp_eval::apply_callable_step;
use crate::nil;
use crate::sexpr::SExpr;
use crate::t;
use crate::types::{Args, EvalResult, Step};

static ENV_COUNT: AtomicUsize = AtomicUsize::new(0);
static ENV_NEXT_ID: AtomicUsize = AtomicUsize::new(0);
static ENV_TRACE_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn current_env_count() -> usize {
    ENV_COUNT.load(Ordering::Relaxed)
}

pub fn env_trace_enable() {
    ENV_TRACE_ENABLED.store(true, Ordering::Relaxed);
}

pub fn env_trace_disable() {
    ENV_TRACE_ENABLED.store(false, Ordering::Relaxed);
}

pub fn env_trace_enabled() -> bool {
    ENV_TRACE_ENABLED.load(Ordering::Relaxed)
}

pub fn env_id(env: &Env) -> usize {
    env.id
}

/// The runtime environment.
///
/// Uses a parent chain with copy-on-write semantics via Arc.
/// Lookup walks up the parent chain, but insertions only modify
/// the local scope (via Arc::make_mut for COW).
#[derive(Clone, Debug, PartialEq)]
pub struct Env {
    pub(crate) id: usize,
    pub(crate) parent: Option<Arc<Env>>,
    pub(crate) local: HashMap<String, OnceLock<SAtom>>,
}

impl Env {
    /// Creates a new root environment with builtins.
    pub fn root() -> Self {
        ENV_COUNT.fetch_add(1, Ordering::Relaxed);
        let id = ENV_NEXT_ID.fetch_add(1, Ordering::Relaxed);

        #[cfg(debug_assertions)]
        let location = format!(
            "at {}:{}",
            std::panic::Location::caller().file(),
            std::panic::Location::caller().line()
        );
        #[cfg(not(debug_assertions))]
        let location = String::new();

        if ENV_TRACE_ENABLED.load(Ordering::Relaxed) {
            eprintln!(
                "[ENV #{} CREATED root {}] count={}",
                id,
                location,
                ENV_COUNT.load(Ordering::Relaxed)
            );
        }

        let mut env = Self {
            id,
            parent: None,
            local: HashMap::new(),
        };
        env.builtins_init();
        env
    }

    /// Creates a new child environment with the given parent.
    #[inline]
    pub fn with_parent(parent: Arc<Env>) -> Self {
        let parent_strong = Arc::strong_count(&parent);
        ENV_COUNT.fetch_add(1, Ordering::Relaxed);

        let id = ENV_NEXT_ID.fetch_add(1, Ordering::Relaxed);

        #[cfg(debug_assertions)]
        let location = format!(
            "at {}:{}",
            std::panic::Location::caller().file(),
            std::panic::Location::caller().line()
        );
        #[cfg(not(debug_assertions))]
        let location = String::new();

        if ENV_TRACE_ENABLED.load(Ordering::Relaxed) {
            eprintln!(
                "[ENV #{} CREATED child {}] parent=#{} parent_strong={} count={}",
                id,
                location,
                parent.id,
                parent_strong,
                ENV_COUNT.load(Ordering::Relaxed)
            );
        }

        Self {
            id,
            parent: Some(parent),
            local: HashMap::new(),
        }
    }

    /// Looks up a name in the environment chain.
    #[inline]
    pub fn lookup(&self, name: &str) -> Option<SAtom> {
        let hashmap_val = self.local.get(name);
        let val_map = hashmap_val.map(|tunk| tunk.get());
        let rec_res = match val_map {
            Some(v) => v.cloned(),
            None => match &self.parent {
                Some(parent) => parent.lookup(name),
                None => None,
            },
        };
        rec_res
    }

    #[inline]
    pub fn record(env: &mut Arc<Env>, name: String) {
        Arc::make_mut(env).local.insert(name, OnceLock::new());
    }

    #[inline]
    pub fn set(env: &Arc<Env>, name: String, value: SAtom) -> Result<(), &'static str> {
        env.local
            .get(&name)
            .ok_or_else(|| "Can't init not recorded")?
            .set(value)
            .or_else(|_| Err("Can't set OnceLock"))?;
        Ok(())
    }

    #[inline]
    pub fn insert(env: &mut Arc<Env>, name: String, value: SAtom) {
        Arc::make_mut(env).local.insert(name, OnceLock::from(value));
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

        self.local
            .insert("nil".into(), OnceLock::from(SAtom::new(nil!())));
        self.local
            .insert("t".into(), OnceLock::from(SAtom::new(t!())));
        self.local.insert(
            "add".into(),
            OnceLock::from(fun_atom(binary_ops(|a, b| a + b))),
        );
        self.local.insert(
            "mul".into(),
            OnceLock::from(fun_atom(binary_ops(|a, b| a * b))),
        );
        self.local.insert(
            "sub".into(),
            OnceLock::from(fun_atom(binary_ops(|a, b| a - b))),
        );
        self.local.insert(
            "div".into(),
            OnceLock::from(fun_atom(binary_ops(|a, b| a / b))),
        );
        self.local
            .insert("car".into(), OnceLock::from(fun_atom(car_op)));
        self.local
            .insert("cdr".into(), OnceLock::from(fun_atom(cdr_op)));
        self.local
            .insert("list".into(), OnceLock::from(fun_atom(list_op)));
        self.local
            .insert("apply".into(), OnceLock::from(fun_atom(apply_op)));
        self.local
            .insert("funcall".into(), OnceLock::from(fun_atom(funcall_op)));
        self.local
            .insert("cons".into(), OnceLock::from(fun_atom(cons_op)));
        self.local
            .insert("eq".into(), OnceLock::from(fun_atom(eq_op)));
    }
}

impl Default for Env {
    fn default() -> Self {
        ENV_COUNT.fetch_add(1, Ordering::Relaxed);
        let id = ENV_NEXT_ID.fetch_add(1, Ordering::Relaxed);

        #[cfg(debug_assertions)]
        let location = format!(
            "at {}:{}",
            std::panic::Location::caller().file(),
            std::panic::Location::caller().line()
        );
        #[cfg(not(debug_assertions))]
        let location = String::new();

        if ENV_TRACE_ENABLED.load(Ordering::Relaxed) {
            eprintln!(
                "[ENV #{} CREATED root {}] count={}",
                id,
                location,
                ENV_COUNT.load(Ordering::Relaxed)
            );
        }

        Self {
            id,
            parent: None,
            local: HashMap::new(),
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let count = ENV_COUNT.fetch_sub(1, Ordering::Relaxed);
        if ENV_TRACE_ENABLED.load(Ordering::Relaxed) {
            #[cfg(debug_assertions)]
            let location = format!(
                "at {}:{}",
                std::panic::Location::caller().file(),
                std::panic::Location::caller().line()
            );
            #[cfg(not(debug_assertions))]
            let location = String::new();

            eprintln!(
                "[ENV #{} DROPPED {}] count={}",
                self.id,
                location,
                count - 1
            );
        }
    }
}
