//! The runtime environment for symbol bindings.
//!
//! The environment maps symbol names to values (`SAtom`). Both ordinary
//! values and functions share the same namespace (`val`), implementing
//! "Option 3: unify the namespaces" as described in the design.
//!
//! Builtin functions are installed as `Atom::Fun` values in the default
//! environment, making them first-class values that can be stored in
//! variables, passed as arguments, and called via `funcall`.

use std::collections::HashMap;
use std::sync::Arc;

use crate::atom::{Atom, Fun, SAtom};
use crate::lisp_eval::apply_callable_step;
use crate::nil;
use crate::sexpr::SExpr;
use crate::t;
use crate::types::{Args, EvalResult, Step};

/// Trait for the evaluator environment.
///
/// This trait abstracts over the environment operations needed by native
/// functions, allowing `Fun::Native` to be defined without a direct
/// reference to the `Env` type (which would create a circular dependency).
pub trait EvaluatorEnv {
    fn lookup(&self, name: &str) -> Option<SAtom>;
    fn insert(&mut self, name: String, value: SAtom);
    fn get_val(&self) -> &HashMap<String, SAtom>;
    fn set_val(&mut self, val: HashMap<String, SAtom>);
}

impl EvaluatorEnv for Env {
    fn lookup(&self, name: &str) -> Option<SAtom> {
        self.val.get(name).cloned()
    }

    fn insert(&mut self, name: String, value: SAtom) {
        self.val.insert(name, value);
    }

    fn get_val(&self) -> &HashMap<String, SAtom> {
        &self.val
    }

    fn set_val(&mut self, val: HashMap<String, SAtom>) {
        self.val = val;
    }
}

/// The runtime environment.
///
/// Maps symbol names to values. Both ordinary values and functions
/// are stored in the same `val` map, enabling unified namespace semantics.
#[derive(Clone)]
pub struct Env {
    pub val: HashMap<String, SAtom>,
}

macro_rules! take_args {
    ($it:expr; $($name:ident),+ $(,)?) => {{
        (|| -> Option<_> {
            let mut iter = ($it).iter();
            $( let $name: SAtom = iter.next()?; )+
            Some(($($name),+))
        })()
    }};
}

/// Counts the number of arguments in an Args value.
#[inline]
fn get_args_count(args: &Args) -> usize {
    match args {
        Args::S(sexpr) => sexpr.len,
        Args::Nil => 0,
    }
}

/// Validates that exactly `n` arguments were provided.
#[inline]
fn expect_exact_args(args: &Args, n: usize, msg: &'static str) -> Result<(), &'static str> {
    if get_args_count(args) == n {
        Ok(())
    } else {
        Err(msg)
    }
}

/// Validates that at least `n` arguments were provided.
#[inline]
fn expect_min_args(args: &Args, n: usize, msg: &'static str) -> Result<(), &'static str> {
    if get_args_count(args) >= n {
        Ok(())
    } else {
        Err(msg)
    }
}

/// Extracts a numeric value from an already-evaluated Atom.
///
/// Returns an error if the atom is not a number.
#[inline]
fn get_num_value(v: &Atom) -> Result<f64, &'static str> {
    match v {
        Atom::Num(n) => Ok(*n),
        _ => Err("expected number"),
    }
}

/// Flattens the last argument of an `apply` call.
///
/// For `(apply f args rest...)`, flattens the final list argument
/// so that all elements become individual arguments.
#[inline]
fn flatten_last_apply_arg(list: &SExpr) -> Result<SAtom, &'static str> {
    let mut items: Vec<SAtom> = list.iter().collect();

    let last = items
        .pop()
        .ok_or("apply expects at least one trailing argument list")?;

    match last.as_ref() {
        Atom::Nil => {}
        Atom::Cons(inner) => items.extend(inner.iter()),
        _ => return Err("last argument to apply must be a list"),
    }

    Ok(SExpr::from_satoms(items))
}

/// Creates a `Fun::Native` from a simple pure function.
///
/// The provided function receives already-evaluated arguments and
/// returns either a result value or an error. This wrapper handles
/// the conversion to the full `Step` type used by the trampoline.
#[inline]
fn native_value<F>(f: F) -> Fun
where
    F: Fn(&mut dyn EvaluatorEnv, &Args) -> Result<SAtom, &'static str> + 'static,
{
    Fun::Native(Box::new(move |env, args, _stack| {
        Ok(Step::Value(f(env, args)?))
    }))
}

/// Wraps a `Fun` in an `Atom::Fun` and then an `SAtom`.
#[inline]
fn fun_atom(fun: Fun) -> SAtom {
    Arc::new(Atom::Fun(Arc::new(fun)))
}

/// Normalizes error messages from callable application.
///
/// Translates the internal "only callable" error to a more user-friendly
/// "first element is not callable" message.
#[inline]
fn normalize_callable_error(r: Result<Step, &'static str>) -> Result<Step, &'static str> {
    match r {
        Err("Only callable values can be used for calling") => Err("first element is not callable"),
        other => other,
    }
}

impl Default for Env {
    fn default() -> Self {
        let binary_ops = |op: fn(f64, f64) -> f64| {
            native_value(move |_: &mut dyn EvaluatorEnv, args: &Args| {
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

        // `car` - returns the first element of a list.
        let car_op = native_value(|_: &mut dyn EvaluatorEnv, args: &Args| {
            expect_exact_args(args, 1, "car expects exactly 1 arg")?;

            let arg = match args {
                Args::S(sexpr) => sexpr.car.as_ref(),
                Args::Nil => unreachable!(),
            };

            match arg {
                Atom::Nil => Ok(nil!().into()),
                Atom::Cons(list) => Ok(list.car.clone()),
                _ => Err("car expects a list"),
            }
        });

        // `cdr` - returns the rest of a list (everything after the first element).
        let cdr_op = native_value(|_: &mut dyn EvaluatorEnv, args: &Args| {
            expect_exact_args(args, 1, "cdr expects exactly 1 arg")?;

            let arg = match args {
                Args::S(sexpr) => sexpr.car.as_ref(),
                Args::Nil => unreachable!(),
            };

            match arg {
                Atom::Nil => Ok(nil!().into()),
                Atom::Cons(list) => Ok(list.cdr.clone()),
                _ => Err("cdr expects a list"),
            }
        });

        // `apply` - applies a function to a list of arguments.
        // Usage: `(apply fn arg1 arg2 ... last-list)`
        // The final argument is flattened into the argument list.
        let apply_op = Fun::Native(Box::new(
            |s: &mut dyn EvaluatorEnv, args: &Args, stack| -> Result<Step, &'static str> {
                let (callable, rest) = match args {
                    Args::S(sexpr) => (sexpr.car.clone(), sexpr.cdr.clone()),
                    Args::Nil => return Err("apply expects at least a function and one list arg"),
                };

                let flat_args_atom = match rest.as_ref() {
                    Atom::Cons(rest_list) => flatten_last_apply_arg(rest_list)?,
                    Atom::Nil => return Err("apply expects at least a function and one list arg"),
                    _ => return Err("apply received invalid argument list"),
                };

                normalize_callable_error(apply_callable_step(callable, flat_args_atom, s, stack))
            },
        ));

        // `funcall` - calls a function with raw arguments (not evaluated).
        // Usage: `(funcall fn arg1 arg2 ...)`
        // Unlike normal function calls, arguments are NOT evaluated before
        // being passed to the function. This is useful for dynamic dispatch.
        let funcall_op = Fun::Native(Box::new(
            |s: &mut dyn EvaluatorEnv, args: &Args, stack| -> Result<Step, &'static str> {
                let (callable, rest) = match args {
                    Args::S(sexpr) => (sexpr.car.clone(), sexpr.cdr.clone()),
                    Args::Nil => return Err("funcall expects at least a function"),
                };

                normalize_callable_error(apply_callable_step(callable, rest, s, stack))
            },
        ));

        // `list` - constructs a list from its arguments.
        let list_op = native_value(|_: &mut dyn EvaluatorEnv, args: &Args| -> EvalResult {
            match args {
                Args::S(sexpr) => Ok(Arc::new(Atom::Cons((*sexpr).clone()))),
                Args::Nil => Ok(SExpr::empty_list()),
            }
        });

        // `cons` - constructs a pair (cons cell).
        let cons_op = native_value(|_: &mut dyn EvaluatorEnv, args: &Args| -> EvalResult {
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

        // `eq` - tests equality of two values (identity comparison).
        let eq_op = native_value(|_: &mut dyn EvaluatorEnv, args: &Args| -> EvalResult {
            expect_exact_args(args, 2, "eq expects exactly 2 args")?;

            match args {
                Args::S(sexpr) => {
                    let (x, y): (SAtom, SAtom) =
                        take_args!(sexpr; x, y).ok_or("eq expects exactly 2 args")?;

                    if x.as_ref() == y.as_ref() {
                        Ok(t!().into())
                    } else {
                        Ok(nil!().into())
                    }
                }
                Args::Nil => unreachable!(),
            }
        });

        let mut val_map = HashMap::new();

        val_map.insert("nil".into(), nil!().into());
        val_map.insert("t".into(), t!().into());

        val_map.insert("add".into(), fun_atom(binary_ops(|a, b| a + b)));
        val_map.insert("mul".into(), fun_atom(binary_ops(|a, b| a * b)));
        val_map.insert("sub".into(), fun_atom(binary_ops(|a, b| a - b)));
        val_map.insert("div".into(), fun_atom(binary_ops(|a, b| a / b)));
        val_map.insert("car".into(), fun_atom(car_op));
        val_map.insert("cdr".into(), fun_atom(cdr_op));
        val_map.insert("list".into(), fun_atom(list_op));
        val_map.insert("apply".into(), fun_atom(apply_op));
        val_map.insert("funcall".into(), fun_atom(funcall_op));
        val_map.insert("cons".into(), fun_atom(cons_op));
        val_map.insert("eq".into(), fun_atom(eq_op));

        Self { val: val_map }
    }
}
