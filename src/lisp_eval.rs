//! The trampolined evaluator for the Lisp interpreter.
//!
//! Uses Arc<Env> for efficient environment management. Function calls save/restore
//! environments via cheap Arc cloning (O(1)) instead of deep-cloning HashMaps (O(n)).

use std::sync::{Arc, OnceLock};

use crate::atom::{Atom, Fun, SAtom, UserFn};
use crate::env::Env;
use crate::sexpr::SExpr;
use crate::types::{Args, Frame, Step};

#[inline]
fn is_special_form(name: &str) -> bool {
    matches!(
        name,
        "lambda" | "quote" | "if" | "def" | "set!" | "labels" | "flet"
    )
}

#[inline]
fn atom_to_args(v: &Atom) -> Result<Args<'_>, &'static str> {
    Args::try_from(v)
}

fn args_to_vec(args: &Atom) -> Result<Vec<SAtom>, &'static str> {
    match args {
        Atom::Nil => Ok(vec![]),
        Atom::Cons(sexpr) => Ok(sexpr.iter().collect()),
        _ => Err("Expected SExpr or Nil"),
    }
}

/// Applies a user-defined function.
/// Uses current environment for closures to support recursive functions.
fn apply_user_fun(
    user: &UserFn,
    args_value: SAtom,
    env: &mut Arc<Env>,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    let actuals = args_to_vec(args_value.as_ref())?;

    if actuals.len() != user.params.len() {
        return Err("wrong number of arguments");
    }

    let saved_env = env.clone();

    let mut new_env = Env::with_parent(user.captured_env.clone());

    for (name, value) in user.params.iter().zip(actuals) {
        new_env.local.insert(name.clone(), OnceLock::from(value));
    }

    *env = Arc::new(new_env);

    stack.push(Frame::RestoreEnv { env: saved_env });

    Ok(Step::Eval(user.body.clone()))
}

fn apply_fun(
    fun: &Fun,
    args_value: SAtom,
    env: &mut Arc<Env>,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    match fun {
        Fun::Native(native) => {
            let args = atom_to_args(args_value.as_ref())?;
            native(env, &args, stack)
        }
        Fun::User(user) => apply_user_fun(user, args_value, env, stack),
    }
}

fn step_from_callable_value(
    callable: SAtom,
    args_value: SAtom,
    env: &mut Arc<Env>,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    match callable.as_ref() {
        Atom::Fun(fun) => apply_fun(fun.as_ref(), args_value, env, stack),
        _ => Err("Only callable values can be used for calling"),
    }
}

#[inline]
fn finish_collected_args(acc_rev: Vec<SAtom>) -> SAtom {
    if acc_rev.is_empty() {
        crate::atom::SAtom::new(Atom::Nil)
    } else {
        SExpr::from_satoms(acc_rev)
    }
}

fn start_eval_args(
    callable: SAtom,
    tail: SAtom,
    env: &mut Arc<Env>,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    match tail.as_ref() {
        Atom::Nil => {
            step_from_callable_value(callable, crate::atom::SAtom::new(Atom::Nil), env, stack)
        }
        Atom::Cons(list) => {
            stack.push(Frame::CollectArgs {
                rest: list.cdr.clone(),
                acc_rev: Vec::with_capacity(list.len),
                callable,
            });
            Ok(Step::Eval(list.car.clone()))
        }
        _ => Err("Expected SExpr or Nil"),
    }
}

fn special_quote(raw_args: SAtom) -> Result<Step, &'static str> {
    match raw_args.as_ref() {
        Atom::Cons(sexpr) if sexpr.len == 1 => Ok(Step::Value(sexpr.car.clone())),
        _ => Err("quote expects exactly 1 arg"),
    }
}

fn special_if(raw_args: SAtom, stack: &mut Vec<Frame>) -> Result<Step, &'static str> {
    match raw_args.as_ref() {
        Atom::Cons(sexpr) if sexpr.len == 3 => {
            let mut it = sexpr.iter();
            let test = it.next().ok_or("if expects exactly 3 args")?;
            let then_branch = it.next().ok_or("if expects exactly 3 args")?;
            let else_branch = it.next().ok_or("if expects exactly 3 args")?;
            stack.push(Frame::BranchIf {
                then_branch,
                else_branch,
            });
            Ok(Step::Eval(test))
        }
        _ => Err("if expects exactly 3 args"),
    }
}

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

fn special_lambda(raw_args: SAtom, env: &Arc<Env>) -> Result<Step, &'static str> {
    match raw_args.as_ref() {
        Atom::Cons(sexpr) if sexpr.len == 2 => {
            let mut it = sexpr.iter();
            let params_val = it
                .next()
                .ok_or("lambda expects exactly 2 args: params and body")?;
            let body_val = it
                .next()
                .ok_or("lambda expects exactly 2 args: params and body")?;
            let params = parse_lambda_params(params_val.as_ref())?;
            // Capture current Arc<Env> for closure
            let user_fn = UserFn {
                params,
                body: body_val,
                captured_env: Arc::new(Env::with_parent(env.clone())),
            };
            Ok(Step::Value(crate::atom::SAtom::new(Atom::Fun(Arc::new(
                Fun::User(user_fn),
            )))))
        }
        _ => Err("lambda expects exactly 2 args: params and body"),
    }
}

fn special_def(
    raw_args: SAtom,
    _env: &mut Arc<Env>,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    match raw_args.as_ref() {
        Atom::Cons(sexpr) if sexpr.len == 2 => {
            let mut it = sexpr.iter();
            let name_val = it.next().ok_or("def expects 2 args: name and value")?;
            let name = match name_val.as_ref() {
                Atom::Sym(s) => s.clone(),
                _ => return Err("def expects a symbol as the first argument"),
            };
            stack.push(Frame::Def { name });
            let cdr_atom = sexpr.cdr.as_ref();
            if let Atom::Cons(value_sexpr) = cdr_atom {
                Ok(Step::Eval(value_sexpr.car.clone()))
            } else {
                Err("def expects a value as second argument")
            }
        }
        _ => Err("def expects exactly 2 args: name and value"),
    }
}

fn special_set(
    raw_args: SAtom,
    _env: &mut Arc<Env>,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    match raw_args.as_ref() {
        Atom::Cons(sexpr) if sexpr.len == 2 => {
            let mut it = sexpr.iter();
            let name_val = it.next().ok_or("set! expects 2 args: name and value")?;
            let name = match name_val.as_ref() {
                Atom::Sym(s) => s.clone(),
                _ => return Err("set! expects a symbol as the first argument"),
            };
            stack.push(Frame::Set { name });
            let cdr_atom = sexpr.cdr.as_ref();
            if let Atom::Cons(value_sexpr) = cdr_atom {
                Ok(Step::Eval(value_sexpr.car.clone()))
            } else {
                Err("set! expects a value as second argument")
            }
        }
        _ => Err("set! expects exactly 2 args: name and value"),
    }
}

fn parse_labels_bindings(raw_args: &Atom) -> Result<Vec<(String, SAtom)>, &'static str> {
    match raw_args {
        Atom::Cons(bindings_list) => {
            let mut result = Vec::new();
            for binding in bindings_list.iter() {
                match binding.as_ref() {
                    Atom::Cons(binding_sexpr) => {
                        if binding_sexpr.len < 2 {
                            return Err("labels binding must have name and body");
                        }
                        let mut it = binding_sexpr.iter();
                        let name_atom = it.next().ok_or("labels binding needs a name")?;
                        let name = match name_atom.as_ref() {
                            Atom::Sym(s) => s.clone(),
                            _ => return Err("labels binding name must be a symbol"),
                        };

                        let params_atom = it.next().ok_or("labels binding needs params")?;
                        let body_atoms: Vec<_> = it.collect();

                        match params_atom.as_ref() {
                            Atom::Cons(_params_sexpr) => {
                                let mut lambda_parts: Vec<SAtom> = Vec::new();
                                lambda_parts.push(crate::atom::SAtom::new(crate::atom::Atom::Sym(
                                    "lambda".to_string(),
                                )));
                                lambda_parts.push(params_atom.clone());
                                lambda_parts.extend(body_atoms);

                                let lambda_sexpr = SExpr::from_satoms(lambda_parts);
                                result.push((name, lambda_sexpr));
                            }
                            _ => return Err("labels binding params must be a list"),
                        }
                    }
                    _ => return Err("labels binding must be a list"),
                }
            }
            Ok(result)
        }
        Atom::Nil => Ok(vec![]),
        _ => Err("labels expects a list of bindings"),
    }
}

fn special_labels(
    raw_args: SAtom,
    env: &mut Arc<Env>,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    match raw_args.as_ref() {
        Atom::Cons(sexpr) => {
            if sexpr.len < 2 {
                return Err("labels expects bindings and body");
            }
            let mut it = sexpr.iter();
            let bindings_val = it.next().ok_or("labels expects bindings and body")?;
            let body_val = it.next().ok_or("labels expects bindings and body")?;

            let bindings = parse_labels_bindings(bindings_val.as_ref())?;

            let saved_env = env.clone();

            for (name, _) in &bindings {
                let placeholder_fn = crate::atom::SAtom::new(Atom::Nil);
                Env::insert(env, name.clone(), placeholder_fn);
            }

            stack.push(Frame::Labels {
                bindings,
                current_idx: 0,
                body: body_val,
                saved_env,
            });

            Ok(Step::Value(crate::atom::SAtom::new(Atom::Nil)))
        }
        _ => Err("labels expects bindings and body"),
    }
}

fn apply_special_form(
    name: &str,
    raw_args: SAtom,
    env: &mut Arc<Env>,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    match name {
        "quote" => special_quote(raw_args),
        "if" => special_if(raw_args, stack),
        "lambda" => special_lambda(raw_args, env),
        "def" => special_def(raw_args, env, stack),
        "set!" => special_set(raw_args, env, stack),
        "labels" => special_labels(raw_args, env, stack),
        _ => Err("Unknown special form"),
    }
}

pub fn apply_callable_step(
    callable: SAtom,
    args_value: SAtom,
    env: &mut Arc<Env>,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    match callable.as_ref() {
        Atom::Fun(_) => step_from_callable_value(callable, args_value, env, stack),
        Atom::Cons(_) | Atom::Sym(_) => {
            stack.push(Frame::ApplyEvaluatedArgs { args_value });
            Ok(Step::Eval(callable))
        }
        _ => Err("first element is not callable"),
    }
}

pub type EvalResult = Result<SAtom, &'static str>;

/// Main evaluation loop (trampoline).
fn drive_step(mut step: Step, env: &mut Arc<Env>, stack: &mut Vec<Frame>) -> EvalResult {
    loop {
        match step {
            Step::Eval(expr) => {
                step = match expr.as_ref() {
                    Atom::Sym(sym) => Step::Value(env.lookup(sym).ok_or("Argument not found")?),
                    Atom::Cons(sexpr) => match sexpr.car.as_ref() {
                        Atom::Sym(fname) if is_special_form(fname) => {
                            apply_special_form(fname, sexpr.cdr.clone(), env, stack)?
                        }
                        _ => {
                            stack.push(Frame::ApplyComputedCallable {
                                tail: sexpr.cdr.clone(),
                            });
                            Step::Eval(sexpr.car.clone())
                        }
                    },
                    _ => Step::Value(expr.clone()),
                };
            }

            Step::Value(value) => match stack.pop() {
                None => return Ok(value),

                Some(Frame::RestoreEnv { env: saved_env }) => {
                    *env = saved_env;
                    step = Step::Value(value);
                }

                Some(Frame::BranchIf {
                    then_branch,
                    else_branch,
                }) => {
                    step = if *value != Atom::Nil {
                        Step::Eval(then_branch)
                    } else {
                        Step::Eval(else_branch)
                    };
                }

                Some(Frame::ApplyEvaluatedArgs { args_value }) => {
                    step = step_from_callable_value(value, args_value, env, stack)?;
                }

                Some(Frame::ApplyComputedCallable { tail }) => {
                    step = start_eval_args(value, tail, env, stack)?;
                }

                Some(Frame::CollectArgs {
                    rest,
                    mut acc_rev,
                    callable,
                }) => {
                    acc_rev.push(value);
                    step = match rest.as_ref() {
                        Atom::Nil => {
                            let args_value = finish_collected_args(acc_rev);
                            step_from_callable_value(callable, args_value, env, stack)?
                        }
                        Atom::Cons(list) => {
                            stack.push(Frame::CollectArgs {
                                rest: list.cdr.clone(),
                                acc_rev,
                                callable,
                            });
                            Step::Eval(list.car.clone())
                        }
                        _ => return Err("Expected SExpr or Nil"),
                    };
                }

                Some(Frame::Def { name }) => {
                    Env::insert(env, name, value.clone());
                    step = Step::Value(value);
                }

                Some(Frame::Set { name }) => {
                    if env.lookup(&name).is_none() {
                        return Err("set!: variable not found");
                    }
                    Env::insert(env, name, value.clone());
                    step = Step::Value(value);
                }

                Some(Frame::Labels {
                    bindings,
                    current_idx,
                    body,
                    saved_env: _,
                }) => {
                    if current_idx == 0 {
                        for (name, _) in &bindings {
                            Env::record(env, name.into());
                        }
                    }
                    if current_idx < bindings.len() {
                        let (name, lambda_val) = &bindings[current_idx].clone();
                        stack.push(Frame::Labels {
                            bindings: bindings,
                            current_idx: current_idx + 1,
                            body,
                            saved_env: env.clone(),
                        });
                        stack.push(Frame::LabelsEvalBody { name: name.clone() });
                        step = Step::Eval(lambda_val.clone());
                    } else {
                        step = Step::Eval(body.clone());
                    }
                }

                Some(Frame::LabelsEvalBody { name }) => {
                    Env::set(env, name, value.clone())?;
                    step = Step::Value(value);
                }
            },
        }
    }
}

/// Applies a callable to a list of already-evaluated arguments.
pub fn apply_callable(callable: SAtom, args_value: SAtom, env: &mut Arc<Env>) -> EvalResult {
    let mut stack = Vec::new();
    let step = apply_callable_step(callable, args_value, env, &mut stack)?;
    drive_step(step, env, &mut stack)
}

/// Evaluates a Lisp expression.
pub fn eval(root: SAtom, env: &mut Arc<Env>) -> EvalResult {
    let mut stack: Vec<Frame> = Vec::new();
    drive_step(Step::Eval(root), env, &mut stack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lisp_parsing::parse;

    fn num(n: f64) -> Atom {
        Atom::Num(n)
    }
    fn sym(s: &str) -> Atom {
        Atom::Sym(s.to_string())
    }
    fn t() -> Atom {
        Atom::T
    }
    fn nil() -> Atom {
        Atom::Nil
    }

    #[test]
    fn test_basic_eval() {
        let env = &mut Arc::new(Env::root());
        let p = parse("1");
        assert_eq!(*eval(p.into(), env).unwrap(), num(1.0));
    }

    #[test]
    fn test_add() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(*eval(parse("(add 3 4 5)").into(), env).unwrap(), num(12.0));
        assert_eq!(
            *eval(parse("(add (add 6 7) 8)").into(), env).unwrap(),
            num(21.0)
        );
    }

    #[test]
    fn test_sub() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(*eval(parse("(sub 10 3)").into(), env).unwrap(), num(7.0));
    }

    #[test]
    fn test_mul() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(*eval(parse("(mul 3 4 5)").into(), env).unwrap(), num(60.0));
    }

    #[test]
    fn test_div() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(*eval(parse("(div 10 2)").into(), env).unwrap(), num(5.0));
    }

    #[test]
    fn test_car() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(
            *eval(parse("(car (quote (1 2 3)))").into(), env).unwrap(),
            num(1.0)
        );
    }

    #[test]
    fn test_cdr() {
        let env = &mut Arc::new(Env::root());
        assert!(matches!(
            eval(parse("(cdr (quote (1 2 3)))").into(), env)
                .unwrap()
                .as_ref(),
            Atom::Cons(_)
        ));
    }

    #[test]
    fn test_cons() {
        let env = &mut Arc::new(Env::root());
        assert!(matches!(
            eval(parse("(cons 1 (quote (2)))").into(), env)
                .unwrap()
                .as_ref(),
            Atom::Cons(_)
        ));
    }

    #[test]
    fn test_eq() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(*eval(parse("(eq 1 1)").into(), env).unwrap(), t());
        assert_eq!(*eval(parse("(eq 1 2)").into(), env).unwrap(), nil());
    }

    #[test]
    fn test_quote() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(*eval(parse("(quote a)").into(), env).unwrap(), sym("a"));
        assert!(matches!(
            eval(parse("(quote (1 2 3))").into(), env).unwrap().as_ref(),
            Atom::Cons(_)
        ));
    }

    #[test]
    fn test_if() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(*eval(parse("(if t 1 2)").into(), env).unwrap(), num(1.0));
        assert_eq!(*eval(parse("(if nil 1 2)").into(), env).unwrap(), num(2.0));
    }

    #[test]
    fn test_nested_if() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(
            *eval(parse("(if (if t t nil) 1 2)").into(), env).unwrap(),
            num(1.0)
        );
        assert_eq!(
            *eval(parse("(if (if nil t nil) 1 2)").into(), env).unwrap(),
            num(2.0)
        );
    }

    #[test]
    fn test_call_lambda() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(
            *eval(parse("((lambda (x) (add x 1)) 5)").into(), env).unwrap(),
            num(6.0)
        );
    }

    #[test]
    fn test_closure_capture() {
        let env = &mut Arc::new(Env::root());
        let parsed = parse("(apply (lambda (x) (add x y)) (list 5))");
        // y is not defined, so this should fail
        assert!(eval(parsed.into(), env).is_err());
    }

    #[test]
    fn test_funcall() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(
            *eval(parse("(funcall add 1 2)").into(), env).unwrap(),
            num(3.0)
        );
    }

    #[test]
    fn test_funcall_with_variable() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(
            *eval(parse("(funcall (lambda (x) (add x 1)) 5)").into(), env).unwrap(),
            num(6.0)
        );
    }

    #[test]
    fn test_user_function() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(
            *eval(
                parse("(apply (lambda (a b) (add a b)) (list 1 2))").into(),
                env
            )
            .unwrap(),
            num(3.0)
        );
    }

    #[test]
    fn test_addmul() {
        let env = &mut Arc::new(Env::root());
        assert_eq!(
            *eval(parse("(add (mul 3 4) 5)").into(), env).unwrap(),
            num(17.0)
        );
    }

    #[test]
    fn test_recursive_fibonacci() {
        let env = &mut Arc::new(Env::root());
        let fib = |n: i32| -> Atom {
            parse(&format!(
                "((lambda (f n) (f f n)) (lambda (f n) (if (eq n 0) 0 (if (eq n 1) 1 (add (f f (sub n 1)) (f f (sub n 2)))))) {})",
                n
            ))
        };
        assert_eq!(*eval(fib(0).into(), env).unwrap(), num(0.0));
        assert_eq!(*eval(fib(1).into(), env).unwrap(), num(1.0));
        assert_eq!(*eval(fib(5).into(), env).unwrap(), num(5.0));
        assert_eq!(*eval(fib(10).into(), env).unwrap(), num(55.0));
    }

    #[test]
    fn test_recursive_factorial() {
        let env = &mut Arc::new(Env::root());
        let fact5 = parse("((lambda (n) ((lambda (FACT) (apply FACT (list FACT n))) (lambda (FACT n) (if (eq n 0) 1 (mul n (apply FACT (list FACT (sub n 1)))))))) 5)");
        assert_eq!(*eval(fact5.into(), env).unwrap(), num(120.0));
    }

    #[test]
    fn test_atom_fun_equality() {
        use std::sync::Arc;
        let fn1 = Fun::Native(Box::new(|_, _, _| Ok(Step::Value(Atom::Nil.into()))));
        let fn2 = Fun::Native(Box::new(|_, _, _| Ok(Step::Value(Atom::Nil.into()))));
        let atom1: Atom = Atom::Fun(Arc::new(fn1));
        let atom2: Atom = Atom::Fun(Arc::new(fn2));
        assert_ne!(atom1, atom2);
    }

    #[test]
    fn test_atom_debug() {
        assert_eq!(format!("{:?}", Atom::Nil), "Nil");
        assert_eq!(format!("{:?}", Atom::T), "T");
        assert_eq!(format!("{:?}", Atom::from(42.0)), "42");
    }

    #[test]
    fn test_sexpr_debug() {
        let list = parse("(1 2 3)");
        let atom: Atom = list.into();
        let debug = format!("{:?}", atom);
        assert!(debug.contains("1"));
    }

    #[test]
    fn test_step_debug() {
        let step = Step::Value(Atom::from(42.0).into());
        let debug = format!("{:?}", step);
        assert!(debug.contains("Value"));
    }
}
