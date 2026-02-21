use crate::{
    atom::{Atom, SAtom},
    cons,
    env::{get_args_from_val, Env},
    sexpr::{SExpr},
};

pub type EvalResult = Result<SAtom, &'static str>;

pub enum Args<'a> {
    S(&'a SExpr),
    Nil,
}

#[derive(Debug)]
pub enum TypeError {
    ExpectedSExprOrNil,
}

impl From<TypeError> for &str {
    fn from(value: TypeError) -> Self {
        match value {
            TypeError::ExpectedSExprOrNil => "Expected SExpr | Nil",
        }
    }
}

impl<'a> TryFrom<&'a Atom> for Args<'a> {
    type Error = TypeError;

    fn try_from(v: &'a Atom) -> Result<Self, Self::Error> {
        match v {
            Atom::Cons(s) => Ok(Args::S(s)),
            Atom::Nil => Ok(Args::Nil),
            _ => Err(TypeError::ExpectedSExprOrNil),
        }
    }
}

pub fn eval(v: SAtom, s: &mut Env) -> EvalResult {
    let eval_body = format!("{:#?}", &*v);
    let res = match &*v {
        Atom::Sym(sym) => Ok(s.val.get(sym).ok_or_else(|| "Argument not found")?.clone()),
        Atom::Cons(SExpr { car, cdr }) => {
            let fname = match &**car {
                Atom::Sym(f) => Ok(f),
                Atom::Fun(fun) => {
                    let args: Result<Args, TypeError> = (&**cdr).try_into();
                    match args {
                        Ok(args) => return fun.call(s, &args),
                        Err(err) => return Err(err.into()),
                    }
                }
                Atom::Cons(_) => {
                    let car_eval = eval(car.clone(), s)?;
                    let eval_res = eval(SAtom::new(cons!(car_eval, cdr.clone())), s);
                    return eval_res;
                }
                _ => Err("Only symbol can be used for calling"),
            }?;

            let funs = s.fun.clone();
            let fun = funs.get(fname).expect("Unknown function");
            let args = get_args_from_val(
                &**cdr,
                s,
                fname != "lambda" && fname != "quote" && fname != "if",
            );

            // println!("Calling {:?} with {:?}", fname, args);
            let v: &Atom = &Atom::Cons(args);
            let args = Args::try_from(v)?;
            let res = fun.call(s, &args);
            // println!("Called {:?} => {:?}", fname, res);
            res
        }
        _ => Ok(v),
    };

    println!("eval: \n{}\n=>{:?}", eval_body, res.clone().unwrap());

    res
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
        let parsed_input = parse("(apply (lambda (a b) (add a b)) 1 2)");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(3));

        let parsed_input = parse("(apply (lambda () (car (list \"good\"))) )");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), str!("good"));

        let parsed_input = parse("(apply (lambda (fun) (apply fun 1)) (lambda (n) (add 1 n)))");
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(2));

        let parsed_input = parse(
            r#"
((lambda (n)
         ((lambda (sub_f) (apply sub_f sub_f n))
                  (lambda (rec n) (if (eq n 0)
                                      0
                                      (apply rec rec (sub n 1))))))
         100)"#,
        );
        assert_eq!(*eval(parsed_input.into(), env).unwrap(), num!(0));

        for fib_n in 5..=15 {
            let parsed_input = parse(&format!(
                r#"
((lambda (n)
   ((lambda (FIB) (apply FIB FIB n)) (lambda (FIB n)
				       (if (eq n 0)
					   0
					 (if (eq n 1)
					     1
					   (add (apply FIB FIB (sub n 1))
						(apply FIB FIB (sub n 2))))))))
 {})"#,
                fib_n
            ));
            assert_eq!(
                *eval(parsed_input.into(), env).unwrap(),
                num!((0..fib_n).fold((0f64, 1f64), |(a, b), _| (b, a + b)).0)
            );
        }
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
