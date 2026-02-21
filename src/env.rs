use std::{collections::HashMap, sync::Arc};

use crate::{
    atom::{Atom, Fun, SAtom, UserFn},
    lisp_eval::{eval, Args, EvalResult},
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
            let r: Option<_> = Some(($($name),+));
            r
        })()
    }};
}

fn get_args_count(args: &Args) -> usize {
    match args {
        Args::S(sexpr) => sexpr.iter().count(),
        Args::Nil => 0,
    }
}

pub fn get_args_from_val(args: &Atom, s: &mut Env, eval_args: bool) -> SExpr {
    match args {
        Atom::Cons(sexpr) => sexpr
            .iter()
            .map(|it| {
                if eval_args {
                    eval(it, s).expect("Coudn't eval arg")
                } else {
                    it
                }
            })
            .collect::<SExpr>(),
        _ => todo!(),
    }
}

fn get_val_form_sym(sname: &str, s: &Env) -> Atom {
    (*s.val[sname]).clone()
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

impl Default for Env {
    fn default() -> Self {
        let mut fun_map: HashMap<String, Fun> = HashMap::new();

        let binary_ops = |op: fn(f64, f64) -> f64| {
            Fun::Native(Box::new(move |s: &mut Env, args: &Args| {
                if get_args_count(args) < 2 {
                    return Err("Expected at least 2 args");
                };
                match args {
                    Args::S(args) => {
                        let mut iter = args.iter();
                        let first = iter.next().ok_or("Expected at least 2 args")?;
                        let mut acc = get_num(first, s)?;

                        for v in iter {
                            let b = get_num(v, s)?;
                            acc = op(acc, b);
                        }

                        Ok(num!(acc).into())
                    }
                    Args::Nil => Err("Calling binary operator with less then 2 args"),
                }
            }))
        };

        let car_op = Fun::Native(Box::new(|s: &mut Env, args: &Args| {
            match args {
                Args::S(sexpr) => {
                    match sexpr.car.as_ref() {
                        // (car <sexpr>)
                        Atom::Cons(SExpr { car, .. }) => Ok(car.clone()),
                        // (car <symbol>)
                        Atom::Sym(sym) => match get_val_form_sym(sym, s) {
                            Atom::Cons(SExpr { car, .. }) => Ok(car),
                            _ => Err("Unsupported type of symbol"),
                        },
                        _ => Err("Unsupported type"),
                    }
                }
                Args::Nil => Ok(nil!().into()),
            }
        }));

        let cdr_op = Fun::Native(Box::new(|s: &mut Env, args: &Args| {
            match args {
                Args::S(args) => {
                    match args.car.as_ref() {
                        // (cdr <sexpr>)
                        Atom::Cons(SExpr { cdr, .. }) => Ok(cdr.clone()),
                        // (cdr <symbol>)
                        Atom::Sym(sym) => match get_val_form_sym(sym, s) {
                            Atom::Cons(SExpr { cdr, .. }) => Ok(cdr),
                            _ => Err("Unsupported type of symbol"),
                        },
                        _ => Err("Unsupported type"),
                    }
                }
                Args::Nil => Ok(nil!().into()),
            }
        }));

        let lambda_op = Fun::Native(Box::new(|s: &mut Env, args: &Args| {
            fn parse_lambda_params(v: &Atom) -> Result<Vec<String>, &'static str> {
                match v {
                    Atom::Cons(param_list) => {
                        let mut out = Vec::new();
                        for p in param_list.iter() {
                            match &*p {
                                Atom::Sym(sname) => out.push(sname.clone()),
                                _ => return Err("lambda params must be symbols"),
                            }
                        }
                        Ok(out)
                    }
                    Atom::Nil => Ok(vec![]),
                    _ => Err("lambda expects param list as first arg"),
                }
            }

            // Expect exactly: (lambda (<params>) <body>)
            if get_args_count(args) != 2 {
                return Err("Expects exactly 2 args: params and body");
            }

            let args = match args {
                Args::S(args) => *args,
                _ => unreachable!(),
            };

            let (params_val, body_val): (SAtom, SAtom) = take_args!(args; params_val, body_val)
                .ok_or_else(|| "Expects exactly 2 args: params and body")?;

            let params = parse_lambda_params(&params_val)?;
            let captured_env: Env = s.clone(); // lexical capture

            let user_fn: UserFn = Box::new((
                body_val.clone(),
                Box::new(
                    move |call_state: &mut Env, call_args: &Args| -> EvalResult {
                        match call_args {
                            Args::S(args) => {
                                if get_args_count(call_args) != params.len() {
                                    return Err("wrong number of arguments");
                                };

                                // Evaluate arguments in caller environment (call-by-value)
                                let evaluated_args = args.iter().map(|a| a).collect::<Vec<_>>();

                                // Switch to lambda lexical env + bound params
                                let saved_env = call_state.val.clone();
                                call_state.val = captured_env.val.clone();

                                for (name, value) in params.iter().zip(evaluated_args.into_iter()) {
                                    call_state.val.insert(name.clone(), value);
                                }

                                let result = eval(body_val.clone(), call_state);

                                // Restore caller env
                                call_state.val = saved_env;

                                result
                            }
                            Args::Nil => {
                                if 0 != params.len() {
                                    return Err("wrong number of arguments");
                                };

                                // Switch to lambda lexical env + bound params
                                let saved_env = call_state.val.clone();
                                call_state.val = captured_env.val.clone();

                                let result = eval(body_val.clone(), call_state);

                                // Restore caller env
                                call_state.val = saved_env;

                                result
                            }
                        }
                    },
                ),
            ));
            Ok(Atom::Fun(Fun::User(user_fn).into()).into())
        }));

        let apply_op = Fun::Native(Box::new(|s: &mut Env, args: &Args| -> EvalResult {
            match args {
                Args::S(SExpr { car, cdr }) => {
                    // car == fun
                    // cdr == args
                    let args = match &**cdr {
                        Atom::Cons(sexpr) => Args::S(sexpr),
                        Atom::Nil => Args::Nil,
                        _ => todo!(),
                    };
                    match &**car {
                        Atom::Fun(fun) => fun.call(s, &args),
                        Atom::Cons(_) => match eval(car.clone(), s)?.as_ref() {
                            Atom::Fun(fun) => fun.call(s, &args),
                            _ => todo!(),
                        },
                        _ => Err("first element is not callable"),
                    }
                }
                Args::Nil => Err("calling apply with no args"),
            }
        }));

        let funcall_op = Fun::Native(Box::new(|s: &mut Env, args: &Args| -> EvalResult {
            match args {
                Args::S(SExpr { car, cdr }) => {
                    // car == fun
                    // cdr == args
                    let args = match &**cdr {
                        Atom::Cons(sexpr) => Args::S(sexpr),
                        Atom::Nil => Args::Nil,
                        _ => todo!(),
                    };
                    match &**car {
                        Atom::Fun(fun) => fun.call(s, &args),
                        Atom::Cons(_) => match eval(car.clone(), s)?.as_ref() {
                            Atom::Fun(fun) => fun.call(s, &args),
                            _ => todo!(),
                        },
                        _ => Err("first element is not callable"),
                    }
                }
                Args::Nil => Err("calling apply with no args"),
            }
        }));

        let list_op = Fun::Native(Box::new(|_: &mut Env, args: &Args| -> EvalResult {
            match args {
                Args::S(sexpr) => Ok(SAtom::new((*sexpr).clone().into())),
                Args::Nil => Ok(nil!().into()),
            }
        }));

        let quote_op = Fun::Native(Box::new(|_: &mut Env, args: &Args| -> EvalResult {
            if get_args_count(args) != 1 {
                return Err("Expects exactly 1 arg");
            }
            match args {
                Args::S(SExpr { car, cdr }) => {
                    if **cdr == Atom::Nil {
                        Ok(car.clone())
                    } else {
                        Err("Expected only 1 arg")
                    }
                }
                Args::Nil => Err("Expected 1 arg"),
            }
        }));

        let cons_op = Fun::Native(Box::new(|_: &mut Env, args: &Args| -> EvalResult {
            if get_args_count(args) != 2 {
                return Err("Expected 2 arg");
            }
            match args {
                Args::S(args) => {
                    let (car, cdr) = take_args!(args; car, cdr).ok_or_else(|| "Expected 2 arg")?;

                    Ok(Atom::Cons(SExpr { car: car, cdr: cdr }).into())
                }
                Args::Nil => Err("Expected 2 arg"),
            }
        }));

        let if_op = Fun::Native(Box::new(|s: &mut Env, args: &Args| -> EvalResult {
            if get_args_count(args) != 3 {
                return Err("Expected 3 arg");
            }
            match args {
                Args::S(sexpr) => {
                    let (test, t_body, f_body) =
                        take_args!(sexpr; test, t_body, f_body).ok_or_else(|| "Expected 3 arg")?;

                    if (*eval(test.clone(), s)?) != Atom::Nil {
                        eval(t_body, s)
                    } else {
                        eval(f_body, s)
                    }
                }
                Args::Nil => Err("Expected 3 arg"),
            }
        }));

        let eq_op = Fun::Native(Box::new(|_: &mut Env, args: &Args| -> EvalResult {
            if get_args_count(&args) != 2 {
                return Err("Expected 2 arg");
            }
            match args {
                Args::S(sexpr) => {
                    let (x, y): (SAtom, SAtom) =
                        take_args!(sexpr; x, y).ok_or_else(|| "Expected 2 arg")?;
                    if &*x == &*y {
                        Ok(t!().into())
                    } else {
                        Ok(nil!().into())
                    }
                }
                Args::Nil => Err("Expected 3 arg"),
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
            fun: fun_map.into(),
            val: val_map,
        }
    }
}
