//! The trampolined evaluator for the Lisp interpreter.
//!
//! This module implements an iterative evaluation loop using the trampoline
//! pattern. Instead of using Rust's call stack for function calls, evaluation
//! proceeds by:
//!
//! 1. Taking a [`Step`] that indicates what to do next (evaluate or return value)
//! 2. Processing the step, potentially pushing [`Frame`]s onto the stack
//! 3. Returning to step 1 with the new step
//!
//! This approach prevents stack overflow on deep recursion and allows
//! native functions to participate in the evaluation loop.
//!
//! ## Special Forms
//!
//! Certain forms require special handling and are not evaluated normally:
//! - `quote`: returns its argument unevaluated
//! - `if`: conditional evaluation
//! - `lambda`: creates a closure
//!
//! ## Function Application
//!
//! Normal function application works as follows:
//! 1. Evaluate the callable (function position) to get a `Fun`
//! 2. Evaluate each argument (already-evaluated by the trampoline)
//! 3. Apply the function with the evaluated arguments
//! 4. For user functions, push a `RestoreEnv` frame to restore the lexical scope

use std::sync::Arc;

use crate::atom::{Atom, Fun, SAtom, UserFn};
use crate::env::EvaluatorEnv;
use crate::sexpr::SExpr;
use crate::types::{Args, Frame, Step};

/// Checks if a symbol name refers to a special form.
#[inline]
fn is_special_form(name: &str) -> bool {
    matches!(name, "lambda" | "quote" | "if")
}

/// Converts an `Atom` to `Args` for native function invocation.
#[inline]
fn atom_to_args(v: &Atom) -> Result<Args<'_>, &'static str> {
    Args::try_from(v)
}

/// Collects the elements of an argument list into a Vec.
fn args_to_vec(args: &Atom) -> Result<Vec<SAtom>, &'static str> {
    match args {
        Atom::Nil => Ok(vec![]),
        Atom::Cons(sexpr) => Ok(sexpr.iter().collect()),
        _ => Err("Expected SExpr or Nil"),
    }
}

/// Applies a user-defined function.
///
/// This function:
/// 1. Validates argument count matches parameter count
/// 2. Saves the current environment
/// 3. Swaps in the captured lexical environment
/// 4. Binds parameters to their argument values
/// 5. Pushes a `RestoreEnv` frame to restore the environment when the body completes
/// 6. Returns `Step::Eval(body)` to evaluate the function body
fn apply_user_fun(
    user: &UserFn,
    args_value: SAtom,
    env: &mut dyn EvaluatorEnv,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    let actuals = args_to_vec(args_value.as_ref())?;

    if actuals.len() != user.params.len() {
        return Err("wrong number of arguments");
    }

    let saved = env.get_val().clone();

    for (name, value) in user.params.iter().zip(actuals) {
        env.insert(name.clone(), value);
    }

    stack.push(Frame::RestoreEnv { saved });

    Ok(Step::Eval(user.body.clone()))
}

/// Applies a function (either native or user-defined).
fn apply_fun(
    fun: &Fun,
    args_value: SAtom,
    env: &mut dyn EvaluatorEnv,
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

/// Converts a callable value to an evaluation step.
///
/// If the callable is already a `Fun`, applies it directly.
/// Otherwise, schedules evaluation of the callable first.
fn step_from_callable_value(
    callable: SAtom,
    args_value: SAtom,
    env: &mut dyn EvaluatorEnv,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    match callable.as_ref() {
        Atom::Fun(fun) => apply_fun(fun.as_ref(), args_value, env, stack),
        _ => Err("Only callable values can be used for calling"),
    }
}

/// Reverses the accumulated argument list and constructs a proper list.
fn finish_collected_args(acc_rev: Vec<SAtom>) -> SAtom {
    if acc_rev.is_empty() {
        crate::atom::SAtom::new(Atom::Nil)
    } else {
        SExpr::from_satoms(acc_rev)
    }
}

/// Initiates argument evaluation for a function call.
///
/// Pushes a `CollectArgs` frame and returns a step to evaluate the first argument.
fn start_eval_args(
    callable: SAtom,
    tail: SAtom,
    env: &mut dyn EvaluatorEnv,
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

/// Handles the `quote` special form.
///
/// Usage: `(quote expr)`
/// Returns `expr` unevaluated.
fn special_quote(raw_args: SAtom) -> Result<Step, &'static str> {
    match raw_args.as_ref() {
        Atom::Cons(sexpr) if sexpr.len == 1 => Ok(Step::Value(sexpr.car.clone())),
        _ => Err("quote expects exactly 1 arg"),
    }
}

/// Handles the `if` special form.
///
/// Usage: `(if test then else)`
/// Pushes a `BranchIf` frame and evaluates the test expression.
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

/// Parses a lambda parameter list into a Vec of parameter names.
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

/// Handles the `lambda` special form.
///
/// Usage: `(lambda (params...) body...)`
/// Creates a closure with the captured lexical environment.
fn special_lambda(raw_args: SAtom, env: &mut dyn EvaluatorEnv) -> Result<Step, &'static str> {
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

            let user_fn = UserFn {
                params,
                body: body_val,
                captured_val: env.get_val().clone(),
            };

            Ok(Step::Value(crate::atom::SAtom::new(Atom::Fun(Arc::new(
                Fun::User(user_fn),
            )))))
        }
        _ => Err("lambda expects exactly 2 args: params and body"),
    }
}

/// Dispatches to the appropriate special form handler.
fn apply_special_form(
    name: &str,
    raw_args: SAtom,
    env: &mut dyn EvaluatorEnv,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    match name {
        "quote" => special_quote(raw_args),
        "if" => special_if(raw_args, stack),
        "lambda" => special_lambda(raw_args, env),
        _ => Err("Unknown special form"),
    }
}

/// Initiates application of a callable with arguments.
///
/// This function handles the case where a callable might need to be
/// evaluated first (e.g., `(f args)` where `f` is a symbol or expression).
pub fn apply_callable_step(
    callable: SAtom,
    args_value: SAtom,
    env: &mut dyn EvaluatorEnv,
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

/// The main evaluation loop (trampoline).
///
/// This function iteratively processes steps until a final value is produced:
/// - `Step::Eval(expr)`: Evaluate the expression (symbol lookup, special forms, function calls)
/// - `Step::Value(v)`: Return to processing the top frame on the stack, or return if stack is empty
fn drive_step(mut step: Step, env: &mut dyn EvaluatorEnv, stack: &mut Vec<Frame>) -> EvalResult {
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

                Some(Frame::RestoreEnv { saved }) => {
                    env.set_val(saved);
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
            },
        }
    }
}

/// Applies a callable to a list of already-evaluated arguments.
///
/// This is the public API for applying functions, used by `apply` and `funcall`.
pub fn apply_callable(
    callable: SAtom,
    args_value: SAtom,
    env: &mut dyn EvaluatorEnv,
) -> EvalResult {
    let mut stack = Vec::new();
    let step = apply_callable_step(callable, args_value, env, &mut stack)?;
    drive_step(step, env, &mut stack)
}

/// Evaluates a Lisp expression in the given environment.
///
/// This is the main entry point for evaluation.
pub fn eval(root: SAtom, env: &mut dyn EvaluatorEnv) -> EvalResult {
    let mut stack: Vec<Frame> = Vec::new();
    drive_step(Step::Eval(root), env, &mut stack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cons, lisp_parsing::parse, nil, num, sexpr, str, sym, t};

    #[test]
    fn test_basic_eval() {
        let env = &mut crate::env::Env::default();
        env.val.insert("a".into(), num!(1).into());
        env.val.insert("b".into(), num!(2).into());

        let parsed_input = parse("a");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(1));

        let parsed_input = parse("b");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(2));

        let parsed_input = parse("(quote a)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), sym!("a"));

        let parsed_input = parse("(quote b)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), sym!("b"));
    }

    #[test]
    fn test_add() {
        let env = &mut crate::env::Env::default();
        let parsed_input = parse("(add 3 4 5)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(12.0));

        let parsed_input = parse("(add (add 6 7) 8)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(21.0));

        let parsed_input = parse("(add 9 (add 10 11))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(30.0));
    }

    #[test]
    fn test_mul() {
        let env = &mut crate::env::Env::default();
        let parsed_input = parse("(mul 1 2)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(2.0));

        let parsed_input = parse("(mul 3 4 5)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(60.0));

        let parsed_input = parse("(mul (mul 6 7) 8)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(336.0));

        let parsed_input = parse("(mul 9 (mul 10 11))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(990.0));
    }

    #[test]
    fn test_addmul() {
        let env = &mut crate::env::Env::default();
        let parsed_input = parse("(add (mul 3 4) 5)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(17.0));

        let parsed_input = parse("(mul (add 3 4) 5)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(35.0));

        let parsed_input = parse("(add 3 (mul 4 5))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(23.0));

        let parsed_input = parse("(mul 3 (add 4 5))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(27.0));
    }

    #[test]
    fn test_sub() {
        let env = &mut crate::env::Env::default();
        let parsed_input = parse("(sub 1 2)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(-1.0));

        let parsed_input = parse("(sub 3 4 5)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(-6.0));

        let parsed_input = parse("(sub (sub 6 7) 8)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(-9.0));

        let parsed_input = parse("(sub 9 (sub 10 11))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(10.0));
    }

    #[test]
    fn test_car() {
        let env = &mut crate::env::Env::default();
        let parsed_input = parse("(car (list 1 (list 2 3 4 5) 6))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(1.0));
    }

    #[test]
    fn test_cdr() {
        let env = &mut crate::env::Env::default();
        let parsed_input = parse("(cdr (list 1 (list 2 3 4 5) (list 6) 7))");
        let res = sexpr!(
            sexpr!(num!(2), num!(3), num!(4), num!(5)),
            sexpr!(num!(6)),
            num!(7),
        );
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), res.into());
    }

    #[test]
    fn test_call_lambda() {
        let env = &mut crate::env::Env::default();
        let parsed_input = parse("(apply (lambda (a b) (add a b)) (list 1 2))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(3));
    }

    #[test]
    fn test_cons() {
        let env = &mut crate::env::Env::default();
        env.val.insert("a".into(), num!(24).into());
        env.val.insert("b".into(), num!(42).into());

        let parsed_input = parse("(cons 1 2)");
        assert_eq!(
            *eval(parsed_input.into(), env).unwrap(),
            cons!(num!(1), num!(2))
        );

        let parsed_input = parse("(cons a b)");
        assert_eq!(
            *eval(parsed_input.into(), env).unwrap(),
            cons!(num!(24), num!(42))
        );

        let parsed_input = parse("(cons (list a) b)");
        assert_eq!(
            *eval(parsed_input.into(), env).unwrap(),
            cons!(sexpr!(num!(24)), num!(42))
        );
        let parsed_input = parse("(cons (list a) (list b))");
        assert_eq!(
            *eval(parsed_input.into(), env).unwrap(),
            cons!(sexpr!(num!(24)), sexpr!(num!(42)))
        );
    }

    #[test]
    fn test_eq() {
        let env = &mut crate::env::Env::default();
        env.val.insert("a".into(), num!(24).into());
        env.val.insert("b".into(), num!(42).into());

        let parsed_input = parse("(eq 1 2)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), nil!());

        let parsed_input = parse("(eq 2 1)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), nil!());

        let parsed_input = parse("(eq 1 1)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), t!());

        let parsed_input = parse("(eq 2 2)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), t!());

        let parsed_input = parse("(eq a b)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), nil!());

        let parsed_input = parse("(eq b a)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), nil!());

        let parsed_input = parse("(eq a a)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), t!());

        let parsed_input = parse("(eq b b)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), t!());

        let parsed_input = parse("(eq (list a b) (quote (24 42)))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), t!());
    }

    #[test]
    fn test_if() {
        let env = &mut crate::env::Env::default();
        env.val.insert("a".into(), num!(24).into());
        env.val.insert("b".into(), num!(42).into());

        let parsed_input = parse("(if (eq t nil) \"TRUE\" \"FALSE\")");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), str!("FALSE"));

        let parsed_input = parse("(if (eq t t) \"TRUE\" \"FALSE\")");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), str!("TRUE"));
    }

    #[test]
    fn test_quote_list() {
        let env = &mut crate::env::Env::default();
        env.val.insert("a".into(), num!(1).into());
        env.val.insert("b".into(), num!(2).into());

        let parsed_input = parse("(quote 1)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(1));

        let parsed_input = parse("(quote (1 2))");
        assert_eq!(
            *eval(parsed_input.into(), env).unwrap(),
            sexpr!(num!(1), num!(2))
        );

        let parsed_input = parse("(list 1)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), sexpr!(num!(1)));

        let parsed_input = parse("(list 1 2)");
        assert_eq!(
            *eval(parsed_input.into(), env).unwrap(),
            sexpr!(num!(1), num!(2))
        );

        let parsed_input = parse("(quote a)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), sym!("a"));

        let parsed_input = parse("(quote (a b))");
        assert_eq!(
            *eval(parsed_input.into(), env).unwrap(),
            sexpr!(sym!("a"), sym!("b"))
        );

        let parsed_input = parse("(list a)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), sexpr!(num!(1)));

        let parsed_input = parse("(list a b)");
        assert_eq!(
            *eval(parsed_input.into(), env).unwrap(),
            sexpr!(num!(1), num!(2))
        );

        let parsed_input = parse("(quote (lambda (a b) (add a b)))");
        assert_eq!(
            *eval(parsed_input.into(), env).unwrap(),
            sexpr!(
                sym!("lambda"),
                sexpr!(sym!("a"), sym!("b")),
                sexpr!(sym!("add"), sym!("a"), sym!("b"))
            )
        );

        let parsed_input = parse("(list (quote (lambda (a b) (add (add a b) b a))) a (quote b))");
        assert_eq!(
            *eval(parsed_input.into(), env).unwrap(),
            sexpr!(
                sexpr!(
                    sym!("lambda"),
                    sexpr!(sym!("a"), sym!("b")),
                    sexpr!(
                        sym!("add"),
                        sexpr!(sym!("add"), sym!("a"), sym!("b")),
                        sym!("b"),
                        sym!("a")
                    )
                ),
                num!(1),
                sym!("b"),
            )
        );
    }

    #[test]
    fn test_funcall() {
        let env = &mut crate::env::Env::default();

        let parsed_input = parse("(funcall car (quote (1 2 3)))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(1));

        let parsed_input = parse("(funcall cdr (quote (1 2 3)))");
        assert_eq!(
            *eval(parsed_input.into(), env).unwrap(),
            sexpr!(num!(2), num!(3))
        );
    }

    #[test]
    fn test_funcall_with_variable() {
        let env = &mut crate::env::Env::default();

        let parsed_input = parse("((lambda (f) (funcall f (quote (1 2)))) car)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(1));
    }

    #[test]
    fn test_apply() {
        let env = &mut crate::env::Env::default();

        let parsed_input = parse("(apply car (quote ((1 2 3))))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(1));

        let parsed_input = parse("(apply add (list 1 2 3))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(6));
    }

    #[test]
    fn test_user_function() {
        let env = &mut crate::env::Env::default();

        let parsed_input = parse("((lambda (x) x) 42)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(42));

        let parsed_input = parse("((lambda (x y) (add x y)) 1 2)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(3));
    }

    #[test]
    fn test_closure_capture() {
        let env = &mut crate::env::Env::default();

        // Test that closures capture lexical environment correctly
        // ((lambda (x) (lambda (y) (add x y))) 10) creates a function that adds 10 to its argument
        // Then call it with 5 to get 15
        let parsed_input = parse("((lambda (x) ((lambda (y) (add x y)) 5)) 10)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(15));
    }

    #[test]
    fn test_nested_if() {
        let env = &mut crate::env::Env::default();

        let parsed_input = parse("(if (if t t nil) 1 2)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(1));

        let parsed_input = parse("(if (if nil t nil) 1 2)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(2));
    }

    #[test]
    fn test_recursive_fibonacci() {
        let env = &mut crate::env::Env::default();

        // Fibonacci using applicative Y combinator (no apply needed):
        // ((lambda (f n) (f f n)) fib-body n)
        // where fib-body = (lambda (f n) (if (eq n 0) 0 (if (eq n 1) 1 (add (f f (sub n 1)) (f f (sub n 2))))))
        let fib = |n: i32| -> Atom {
            parse(&format!(
                "((lambda (f n) (f f n)) (lambda (f n) (if (eq n 0) 0 (if (eq n 1) 1 (add (f f (sub n 1)) (f f (sub n 2)))))) {})",
                n
            ))
        };

        assert_eq!(*eval(fib(0).into(), env).unwrap(), num!(0));
        assert_eq!(*eval(fib(1).into(), env).unwrap(), num!(1));
        assert_eq!(*eval(fib(5).into(), env).unwrap(), num!(5));
        assert_eq!(*eval(fib(10).into(), env).unwrap(), num!(55));
    }

    #[test]
    fn test_recursive_factorial() {
        let env = &mut crate::env::Env::default();

        // Factorial using Y combinator
        let fact5 = parse("((lambda (n) ((lambda (FACT) (apply FACT (list FACT n))) (lambda (FACT n) (if (eq n 0) 1 (mul n (apply FACT (list FACT (sub n 1)))))))) 5)");
        assert_eq!(*eval(fact5.into(), env).unwrap(), num!(120));

        let fact0 = parse("((lambda (n) ((lambda (FACT) (apply FACT (list FACT n))) (lambda (FACT n) (if (eq n 0) 1 (mul n (apply FACT (list FACT (sub n 1)))))))) 0)");
        assert_eq!(*eval(fact0.into(), env).unwrap(), num!(1));
    }
}
