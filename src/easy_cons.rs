#[macro_export]
macro_rules! num {
    ($x:expr) => {{
        use crate::atom::Atom;
        let r: Atom = Atom::Num(($x).into());
        r
    }};
}
#[macro_export]
macro_rules! str {
    ($x:expr) => {{
        let r: Atom = Atom::Str(($x).into());
        r
    }};
}
#[macro_export]
macro_rules! sym {
    ($x:expr) => {{
        use crate::atom::Atom;
        let r: Atom = Atom::Sym(($x).into());
        r
    }};
}
#[macro_export]
macro_rules! nil {
    () => {{
        use crate::atom::Atom;
        let r: Atom = Atom::Nil;
        r
    }};
}
#[macro_export]
macro_rules! t {
    () => {{
        use crate::atom::Atom;
        let r: Atom = Atom::T;
        r
    }};
}
#[macro_export]
macro_rules! sexpr {
    // empty list
    () => {
        use crate::atom::Atom;
        Atom::Nil
    };

    // one or more elements (atoms or vals), comma-separated
    ($($x:expr),+ $(,)?) => {{
        use crate::atom::Atom;
        let r: Atom = Atom::Cons(vec![ $( ($x) ),+ ].into_iter().collect());
        r
    }};
}

#[macro_export]
macro_rules! cons {
    // two elements (atoms or vals), comma-separated
    ($car:expr, $cdr:expr $(,)?) => {{
        use crate::atom::Atom;
        use crate::sexpr::SExpr;
        let r: Atom = Atom::Cons(SExpr {
            car: ($car).into(),
            cdr: ($cdr).into(),
        });
        r
    }};
}
