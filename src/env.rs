use std::{collections::HashMap, sync::Arc};

use crate::{
    atom::{Atom, Fun, SAtom, UserFn},
    lisp_eval::{apply_callable, eval, Args, EvalResult},
    nil, num,
    sexpr::SExpr,
    t,
};

#[derive(Clone)]
pub struct Env {
    pub val: HashMap<String, SAtom>,
    pub fun: Arc<HashMap<String, Fun>>,
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
fn atom_to_args(atom: &Atom) -> Result<Args, &'static str> {
    match atom {
        Atom::Nil => Ok(Args::Nil),
        Atom::Cons(sexpr) => Ok(Args::S(sexpr)),
        _ => Err("expected list"),
    }
}

#[inline]
fn resolve_value<'a>(arg: &'a Atom, env: &'a Env) -> Option<&'a Atom> {
    match arg {
        Atom::Sym(sym) => env.val.get(sym).map(|v| v.as_ref()),
        other => Some(other),
    }
}

#[inline]
fn get_val_from_sym<'a>(sname: &str, s: &'a Env) -> Option<&'a Atom> {
    s.val.get(sname).map(|v| v.as_ref())
}

pub fn get_args_from_val(args: &Atom, s: &mut Env, eval_args: bool) -> SAtom {
    match args {
        Atom::Nil => SExpr::empty_list(),
        Atom::Cons(sexpr) => SExpr::from_satoms(sexpr.iter().map(|it| {
            if eval_args {
                eval(it, s).expect("Couldn't eval arg")
            } else {
                it
            }
        })),
        _ => panic!("expected list"),
    }
}

pub fn get_num(v: SAtom, s: &mut Env) -> Result<f64, &'static str> {
    match &*v {
        Atom::Num(n) => Ok(*n),

        Atom::Sym(sym) => {
            let bound = s.val.get(sym).ok_or("Unknown symbol")?;
            match bound.as_ref() {
                Atom::Num(n) => Ok(*n),
                _ => Err("Unsupported variable type"),
            }
        }

        Atom::Cons(_) => match *eval(v, s)? {
            Atom::Num(n) => Ok(n),
            _ => Err("Unsupported type"),
        },

        _ => Err("Unsupported type"),
    }
}

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

#[inline]
fn args_to_atom(args: &Args) -> SAtom {
    match args {
        Args::Nil => Arc::new(Atom::Nil),
        Args::S(sexpr) => Arc::new(Atom::Cons((*sexpr).clone())),
    }
}

#[inline]
fn call_callable(s: &mut Env, callable: &SAtom, args: &Args) -> EvalResult {
    let callable_value = match callable.as_ref() {
        Atom::Fun(_) => callable.clone(),

        Atom::Cons(_) | Atom::Sym(_) => eval(callable.clone(), s)?,

        _ => return Err("first element is not callable"),
    };

    match apply_callable(callable_value, args_to_atom(args), s) {
        Err("Only callable values can be used for calling") => {
            Err("first element is not callable")
        }
        other => other,
    }
}

impl Default for Env {
    fn default() -> Self {
        let mut fun_map: HashMap<String, Fun> = HashMap::new();

        let binary_ops = |op: fn(f64, f64) -> f64| {
            Fun::Native(Box::new(move |s: &mut Env, args: &Args| {
                expect_min_args(args, 2, "expected at least 2 args")?;

                match args {
                    Args::S(args) => {
                        let mut iter = args.iter();
                        let first = iter.next().ok_or("expected at least 2 args")?;
                        let mut acc = get_num(first, s)?;

                        for v in iter {
                            acc = op(acc, get_num(v, s)?);
                        }

                        Ok(num!(acc).into())
                    }
                    Args::Nil => Err("expected at least 2 args"),
                }
            }))
        };

        let car_op = Fun::Native(Box::new(|s: &mut Env, args: &Args| {
            expect_exact_args(args, 1, "car expects exactly 1 arg")?;

            let arg = match args {
                Args::S(sexpr) => sexpr.car.as_ref(),
                Args::Nil => unreachable!(),
            };

            match resolve_value(arg, s) {
                Some(Atom::Nil) => Ok(nil!().into()),
                Some(Atom::Cons(list)) => Ok(list.car.clone()),
                _ => Err("car expects a list"),
            }
        }));

        let cdr_op = Fun::Native(Box::new(|s: &mut Env, args: &Args| {
            expect_exact_args(args, 1, "cdr expects exactly 1 arg")?;

            let arg = match args {
                Args::S(sexpr) => sexpr.car.as_ref(),
                Args::Nil => unreachable!(),
            };

            match resolve_value(arg, s) {
                Some(Atom::Nil) => Ok(nil!().into()),
                Some(Atom::Cons(list)) => Ok(list.cdr.clone()),
                _ => Err("cdr expects a list"),
            }
        }));

        let lambda_op = Fun::Native(Box::new(|s: &mut Env, args: &Args| {
            fn parse_lambda_params(v: &Atom) -> Result<Vec<String>, &'static str> {
                match v {
                    Atom::Cons(param_list) => {
                        let mut out = Vec::new();
                        for p in param_list.iter() {
                            match p.as_ref() {
                                Atom::Sym(name) => out.push(name.clone()),
                                _ => return Err("lambda params must be symbols"),
                            }
                        }
                        Ok(out)
                    }
                    Atom::Nil => Ok(vec![]),
                    _ => Err("lambda expects param list as first arg"),
                }
            }

            expect_exact_args(args, 2, "lambda expects exactly 2 args: params and body")?;

            let args = match args {
                Args::S(args) => *args,
                Args::Nil => unreachable!(),
            };

            let (params_val, body_val): (SAtom, SAtom) =
                take_args!(args; params_val, body_val)
                    .ok_or("lambda expects exactly 2 args: params and body")?;

            let params = parse_lambda_params(params_val.as_ref())?;

            let user_fn = UserFn {
                params,
                body: body_val,
                captured_val: s.val.clone(),
            };

            Ok(Arc::new(Atom::Fun(Arc::new(Fun::User(user_fn)))))
        }));

        let apply_op = Fun::Native(Box::new(|s: &mut Env, args: &Args| -> EvalResult {
            let (callable, rest) = match args {
                Args::S(sexpr) => (sexpr.car.clone(), sexpr.cdr.clone()),
                Args::Nil => return Err("apply expects at least a function and one list arg"),
            };

            let flat_args_atom = match rest.as_ref() {
                Atom::Cons(rest_list) => flatten_last_apply_arg(rest_list)?,
                Atom::Nil => return Err("apply expects at least a function and one list arg"),
                _ => return Err("apply received invalid argument list"),
            };

            let flat_args = atom_to_args(flat_args_atom.as_ref())?;
            call_callable(s, &callable, &flat_args)
        }));

        let funcall_op = Fun::Native(Box::new(|s: &mut Env, args: &Args| -> EvalResult {
            let (callable, rest) = match args {
                Args::S(sexpr) => (sexpr.car.clone(), sexpr.cdr.clone()),
                Args::Nil => return Err("funcall expects at least a function"),
            };

            let fn_args = atom_to_args(rest.as_ref())?;
            call_callable(s, &callable, &fn_args)
        }));

        let list_op = Fun::Native(Box::new(|_: &mut Env, args: &Args| -> EvalResult {
            match args {
                Args::S(sexpr) => Ok(Arc::new(Atom::Cons((*sexpr).clone()))),
                Args::Nil => Ok(SExpr::empty_list()),
            }
        }));

        let quote_op = Fun::Native(Box::new(|_: &mut Env, args: &Args| -> EvalResult {
            expect_exact_args(args, 1, "quote expects exactly 1 arg")?;

            match args {
                Args::S(sexpr) => Ok(sexpr.car.clone()),
                Args::Nil => unreachable!(),
            }
        }));

        let cons_op = Fun::Native(Box::new(|_: &mut Env, args: &Args| -> EvalResult {
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
        }));

        let if_op = Fun::Native(Box::new(|s: &mut Env, args: &Args| -> EvalResult {
            expect_exact_args(args, 3, "if expects exactly 3 args")?;

            match args {
                Args::S(sexpr) => {
                    let (test, t_body, f_body) = take_args!(sexpr; test, t_body, f_body)
                        .ok_or("if expects exactly 3 args")?;

                    if *eval(test, s)? != Atom::Nil {
                        eval(t_body, s)
                    } else {
                        eval(f_body, s)
                    }
                }
                Args::Nil => unreachable!(),
            }
        }));

        let eq_op = Fun::Native(Box::new(|_: &mut Env, args: &Args| -> EvalResult {
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
        }));

        fun_map.insert("add".into(), binary_ops(|a, b| a + b));
        fun_map.insert("mul".into(), binary_ops(|a, b| a * b));
        fun_map.insert("sub".into(), binary_ops(|a, b| a - b));
        fun_map.insert("div".into(), binary_ops(|a, b| a / b));
        fun_map.insert("car".into(), car_op);
        fun_map.insert("cdr".into(), cdr_op);
        fun_map.insert("list".into(), list_op);
        fun_map.insert("quote".into(), quote_op);
        fun_map.insert("lambda".into(), lambda_op);
        fun_map.insert("apply".into(), apply_op);
        fun_map.insert("funcall".into(), funcall_op);
        fun_map.insert("cons".into(), cons_op);
        fun_map.insert("if".into(), if_op);
        fun_map.insert("eq".into(), eq_op);

        let mut val_map = HashMap::new();
        val_map.insert("nil".into(), nil!().into());
        val_map.insert("t".into(), t!().into());

        Self {
            fun: Arc::new(fun_map),
            val: val_map,
        }
    }
}
