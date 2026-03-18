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
        use crate::atom::Atom;
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
    () => {{
        use crate::atom::Atom;
        Atom::Nil
    }};

    // one or more elements
    ($($x:expr),+ $(,)?) => {{
        use crate::sexpr::SExpr;
        (*SExpr::from_atoms(vec![$($x),+])).clone()
    }};
}

#[macro_export]
macro_rules! cons {
    ($car:expr, $cdr:expr $(,)?) => {{
        use crate::atom::Atom;
        use crate::sexpr::SExpr;

        let car = $car;
        let cdr = $cdr;

        let len = 1 + match &cdr {
            Atom::Nil => 0,
            Atom::Cons(sexpr) => sexpr.len,
            _ => 1,
        };

        Atom::Cons(SExpr {
            car: car.into(),
            cdr: cdr.into(),
            len,
        })
    }};
}
