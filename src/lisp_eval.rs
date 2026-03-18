use std::{collections::HashMap, sync::Arc};

use crate::{
    atom::{Atom, Fun, SAtom, UserFn},
    env::Env,
    sexpr::SExpr,
};

pub type EvalResult = Result<SAtom, &'static str>;

pub enum Args<'a> {
    S(&'a SExpr),
    Nil,
}

impl<'a> TryFrom<&'a Atom> for Args<'a> {
    type Error = &'static str;

    fn try_from(v: &'a Atom) -> Result<Self, Self::Error> {
        match v {
            Atom::Cons(s) => Ok(Args::S(s)),
            Atom::Nil => Ok(Args::Nil),
            _ => Err("Expected SExpr | Nil"),
        }
    }
}

#[inline]
fn atom_to_args(v: &Atom) -> Result<Args<'_>, &'static str> {
    Args::try_from(v)
}

#[inline]
fn is_special_form(name: &str) -> bool {
    matches!(name, "lambda" | "quote" | "if")
}

fn eval_call_args(tail: &Atom, env: &mut Env, eval_args: bool) -> EvalResult {
    match tail {
        Atom::Nil => Ok(Arc::new(Atom::Nil)),

        Atom::Cons(sexpr) if !eval_args => Ok(Arc::new(Atom::Cons((*sexpr).clone()))),

        Atom::Cons(sexpr) => {
            let mut items = Vec::with_capacity(sexpr.len);
            for it in sexpr.iter() {
                items.push(eval(it, env)?);
            }
            Ok(SExpr::from_satoms(items))
        }

        _ => Err("Expected SExpr | Nil"),
    }
}

fn args_to_vec(args: &Atom) -> Result<Vec<SAtom>, &'static str> {
    match args {
        Atom::Nil => Ok(vec![]),
        Atom::Cons(sexpr) => Ok(sexpr.iter().collect()),
        _ => Err("Expected SExpr | Nil"),
    }
}

enum Frame {
    ApplyComputedCallable { tail: SAtom },
    RestoreEnv { saved: HashMap<String, SAtom> },
}

enum Step {
    Value(SAtom),
    Eval(SAtom),
}

fn apply_user_fun(
    user: &UserFn,
    args_value: SAtom,
    env: &mut Env,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    let actuals = args_to_vec(args_value.as_ref())?;

    if actuals.len() != user.params.len() {
        return Err("wrong number of arguments");
    }

    let saved = std::mem::replace(&mut env.val, user.captured_val.clone());

    for (name, value) in user.params.iter().zip(actuals) {
        env.val.insert(name.clone(), value);
    }

    stack.push(Frame::RestoreEnv { saved });

    Ok(Step::Eval(user.body.clone()))
}

fn apply_fun(
    fun: &Fun,
    args_value: SAtom,
    env: &mut Env,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    match fun {
        Fun::Native(_) => {
            let args = atom_to_args(args_value.as_ref())?;
            Ok(Step::Value(fun.call(env, &args)?))
        }
        Fun::User(user) => apply_user_fun(user, args_value, env, stack),
    }
}

fn eval_symbol_call(
    fname: &str,
    tail: &Atom,
    env: &mut Env,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    let funs = Arc::clone(&env.fun);
    let bound_value = env.val.get(fname).cloned();

    let evaluated_args = eval_call_args(tail, env, !is_special_form(fname))?;

    if let Some(fun) = funs.get(fname) {
        return apply_fun(fun, evaluated_args, env, stack);
    }

    match bound_value.as_deref() {
        Some(Atom::Fun(fun)) => apply_fun(fun.as_ref(), evaluated_args, env, stack),
        _ => Err("Unknown function"),
    }
}

fn step_from_callable_value(
    callable: SAtom,
    args_value: SAtom,
    env: &mut Env,
    stack: &mut Vec<Frame>,
) -> Result<Step, &'static str> {
    match callable.as_ref() {
        Atom::Fun(fun) => apply_fun(fun.as_ref(), args_value, env, stack),
        _ => Err("Only callable values can be used for calling"),
    }
}

fn drive_step(mut step: Step, env: &mut Env, stack: &mut Vec<Frame>) -> EvalResult {
    'eval_loop: loop {
        match step {
            Step::Eval(expr) => {
                step = match expr.as_ref() {
                    Atom::Sym(sym) => {
                        Step::Value(env.val.get(sym).ok_or("Argument not found")?.clone())
                    }

                    Atom::Cons(sexpr) => match sexpr.car.as_ref() {
                        Atom::Sym(fname) => {
                            eval_symbol_call(fname, sexpr.cdr.as_ref(), env, stack)?
                        }

                        _ => {
                            stack.push(Frame::ApplyComputedCallable {
                                tail: sexpr.cdr.clone(),
                            });
                            step = Step::Eval(sexpr.car.clone());
                            continue 'eval_loop;
                        }
                    },

                    _ => Step::Value(expr.clone()),
                };
            }

            Step::Value(value) => match stack.pop() {
                None => return Ok(value),

                Some(Frame::RestoreEnv { saved }) => {
                    env.val = saved;
                    step = Step::Value(value);
                }

                Some(Frame::ApplyComputedCallable { tail }) => {
                    let evaluated_args = eval_call_args(tail.as_ref(), env, true)?;
                    step = step_from_callable_value(value, evaluated_args, env, stack)?;
                }
            },
        }
    }
}

pub fn apply_callable(callable: SAtom, args_value: SAtom, env: &mut Env) -> EvalResult {
    let mut stack = Vec::new();
    let step = step_from_callable_value(callable, args_value, env, &mut stack)?;
    drive_step(step, env, &mut stack)
}

pub fn eval(root: SAtom, env: &mut Env) -> EvalResult {
    let mut stack: Vec<Frame> = Vec::new();
    drive_step(Step::Eval(root), env, &mut stack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cons, lisp_parsing::parse, nil, num, sexpr, str, sym, t};

    #[test]
    fn test_basic_eval() {
        let env = &mut Env::default();
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
        let env = &mut Env::default();
        let parsed_input = parse("(add 3 4 5)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(12.0));

        let parsed_input = parse("(add (add 6 7) 8)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(21.0));

        let parsed_input = parse("(add 9 (add 10 11))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(30.0));
    }

    #[test]
    fn test_mul() {
        let env = &mut Env::default();
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
        let env = &mut Env::default();
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
        let env = &mut Env::default();
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
        let env = &mut Env::default();
        let parsed_input = parse("(car (list 1 (list 2 3 4 5) 6))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(1.0));
    }

    #[test]
    fn test_cdr() {
        let env = &mut Env::default();
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
        let env = &mut Env::default();
        let parsed_input = parse("(apply (lambda (a b) (add a b)) (list 1 2))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(3));

        // let parsed_input = parse("(apply (lambda () (car (list \"good\"))) nil)");
        //         assert_eq!(*eval(parsed_input.into(), env).unwrap(), str!("good"));

        //         let parsed_input = parse("(apply (lambda (fun) (apply fun (list 1))) (list (lambda (n) (add 1 n))))");
        //         assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(2));

        //         let parsed_input = parse(
        //             r#"
        // ((lambda (n)
        //          ((lambda (sub_f) (apply sub_f (list sub_f n)))
        //                   (lambda (rec n) (if (eq n 0)
        //                                       0
        //                                       (apply rec (list rec (sub n 1)))))))
        //          100)"#,
        //         );
        //         assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(0));

        //         for fib_n in 0..=15 {
        //             let parsed_input = parse(&format!(
        //                 r#"
        // ((lambda (n)
        //    ((lambda (FIB) (apply FIB (list FIB n))) (lambda (FIB n)
        // 				       (if (eq n 0)
        // 					   0
        // 					 (if (eq n 1)
        // 					     1
        // 					   (add (apply FIB (list FIB (sub n 1)))
        // 						(apply FIB (list FIB (sub n 2)))))))))
        //  {})"#,
        //                 fib_n
        //             ));
        //             assert_eq!(
        //                 *eval(parsed_input.into(), env).unwrap(),
        //                 num!((0..fib_n).fold((0f64, 1f64), |(a, b), _| (b, a + b)).0)
        //             );
        //         }
    }

    #[test]
    fn test_cons() {
        let env = &mut Env::default();
        env.val.insert("a".into(), num!(24).into());
        env.val.insert("b".into(), num!(42).into());

        let parsed_input = parse("(cons 1 2)");
        assert_eq!(
            *eval(parsed_input.into(), env).unwrap(),
            cons!(num!(1), num!(2))
        );

        let parsed_input = parse("(cons (quote a) (quote b))");
        assert_eq!(
            *eval(parsed_input.into(), env).unwrap(),
            cons!(sym!("a"), sym!("b"))
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
        let env = &mut Env::default();
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
        let env = &mut Env::default();
        env.val.insert("a".into(), num!(24).into());
        env.val.insert("b".into(), num!(42).into());

        let parsed_input = parse("(if (eq t nil) \"TRUE\" \"FALSE\")");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), str!("FALSE"));

        let parsed_input = parse("(if (eq t t) \"TRUE\" \"FALSE\")");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), str!("TRUE"));
    }

    #[test]
    fn test_quote_list() {
        let env = &mut Env::default();
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
}
