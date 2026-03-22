use crate::atom::SAtom;
use std::sync::Arc;

pub type EvalResult = Result<SAtom, &'static str>;

#[derive(Clone)]
pub enum Frame {
    ApplyComputedCallable {
        tail: SAtom,
    },
    ApplyEvaluatedArgs {
        args_value: SAtom,
    },
    CollectArgs {
        rest: SAtom,
        acc_rev: Vec<SAtom>,
        callable: SAtom,
    },
    BranchIf {
        then_branch: SAtom,
        else_branch: SAtom,
    },
    RestoreEnv {
        env: Arc<crate::env::Env>,
    },
    Def {
        name: String,
    },
    Set {
        name: String,
    },
    Labels {
        bindings: Vec<(String, crate::atom::SAtom)>,
        current_idx: usize,
        body: SAtom,
        saved_env: Arc<crate::env::Env>,
    },
    LabelsEvalBody {
        name: String,
    },
}

#[derive(Debug)]
pub enum Step {
    Eval(SAtom),
    Value(SAtom),
}

#[derive(Debug, Clone, Copy)]
pub enum Args<'a> {
    S(&'a crate::sexpr::SExpr),
    Nil,
}

impl<'a> TryFrom<&'a crate::atom::Atom> for Args<'a> {
    type Error = &'static str;
    fn try_from(v: &'a crate::atom::Atom) -> Result<Self, Self::Error> {
        match v {
            crate::atom::Atom::Cons(s) => Ok(Args::S(s)),
            crate::atom::Atom::Nil => Ok(Args::Nil),
            _ => Err("Expected SExpr | Nil"),
        }
    }
}
