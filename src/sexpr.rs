use std::{
    fmt,
    fmt::{Debug, Formatter},
    // sync::Arc,
};

use crate::atom::{Atom, SAtom};

#[derive(PartialEq, Clone)]
pub struct SExpr {
    pub car: SAtom,
    pub cdr: SAtom,
}

impl Debug for SExpr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        fn fmt_val(v: &Atom, f: &mut Formatter<'_>, indent: usize, pretty: bool) -> fmt::Result {
            match v {
                Atom::Cons(sexpr) => fmt_sexpr(sexpr, f, indent, pretty),
                _ => v.fmt(f),
            }
        }

        fn collect(sexpr: &SExpr) -> (Vec<SAtom>, Option<SAtom>) {
            let mut elems = vec![sexpr.car.clone()];
            let mut cur = sexpr.cdr.clone();

            loop {
                match cur.as_ref() {
                    Atom::Nil => return (elems, None),
                    Atom::Cons(cell) => {
                        elems.push(cell.car.clone());
                        cur = cell.cdr.clone();
                    }
                    _ => return (elems, Some(cur)),
                }
            }
        }

        fn fmt_sexpr(
            sexpr: &SExpr,
            f: &mut Formatter<'_>,
            indent: usize,
            pretty: bool,
        ) -> fmt::Result {
            let (elems, tail) = collect(sexpr);

            write!(f, "(")?;
            if elems.is_empty() {
                return write!(f, ")");
            }

            if !pretty {
                for (i, v) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    fmt_val(v.as_ref(), f, indent, false)?;
                }
                if let Some(t) = tail {
                    write!(f, " . ")?;
                    fmt_val(t.as_ref(), f, indent, false)?;
                }
                return write!(f, ")");
            }

            // Emacs-ish pretty formatting:
            // (head first
            //       next
            //       ...)
            let head_inline = format!("{:?}", elems[0].as_ref());
            write!(f, "{head_inline}")?;

            if elems.len() == 1 {
                if let Some(t) = tail {
                    write!(f, " . ")?;
                    fmt_val(t.as_ref(), f, indent + 2, true)?;
                }
                return write!(f, ")");
            }

            let align = indent + 1 + head_inline.len() + 1;

            write!(f, " ")?;
            fmt_val(elems[1].as_ref(), f, align, true)?;

            for v in elems.iter().skip(2) {
                write!(f, "\n{:width$}", "", width = align)?;
                fmt_val(v.as_ref(), f, align, true)?;
            }

            if let Some(t) = tail {
                write!(f, "\n{:width$}. ", "", width = align)?;
                fmt_val(t.as_ref(), f, align + 2, true)?;
            }

            // close paren immediately (no extra newline), so nested closes become ))))
            write!(f, ")")
        }

        fmt_sexpr(self, f, 0, f.alternate())
    }
}

impl SExpr {
    pub fn iter(&self) -> SExprIter {
        SExprIter {
            cursor: Some(SAtom::new(Atom::Cons(self.clone()))),
        }
    }
}

impl FromIterator<Atom> for SExpr {
    fn from_iter<T: IntoIterator<Item = Atom>>(iter: T) -> Self {
        // Adjust this if your list terminator is different.
        let nil = Atom::Nil;

        let mut items: Vec<Atom> = iter.into_iter().collect();

        // If your language represents the empty list as an Atom (nil),
        // an "empty" SExpr is a sentinel. If you don't want this,
        // you can `panic!` here instead.
        if items.is_empty() {
            return SExpr {
                car: SAtom::new(nil.clone()),
                cdr: SAtom::new(nil),
            };
        }

        // Build a proper list by cons-ing from the end.
        let mut tail: Atom = nil;
        while let Some(v) = items.pop() {
            let cell = SExpr {
                car: SAtom::new(v),
                cdr: SAtom::new(tail),
            };
            tail = Atom::Cons(cell);
        }

        match tail {
            Atom::Cons(se) => se,
            _ => unreachable!(),
        }
    }
}

impl FromIterator<SAtom> for SExpr {
    fn from_iter<T: IntoIterator<Item = SAtom>>(iter: T) -> Self {
        // Adjust this if your list terminator is different.
        let nil = Atom::Nil;

        let mut items: Vec<SAtom> = iter.into_iter().collect();

        // If your language represents the empty list as an Atom (nil),
        // an "empty" SExpr is a sentinel. If you don't want this,
        // you can `panic!` here instead.
        if items.is_empty() {
            return SExpr {
                car: SAtom::new(nil.clone()),
                cdr: SAtom::new(nil),
            };
        }

        // Build a proper list by cons-ing from the end.
        let mut tail: Atom = nil;
        while let Some(v) = items.pop() {
            let cell = SExpr {
                car: v,
                cdr: tail.into(),
            };
            tail = Atom::Cons(cell);
        }

        match tail {
            Atom::Cons(se) => se,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SExprIter {
    cursor: Option<SAtom>,
}

impl Iterator for SExprIter {
    type Item = SAtom;

    fn next(&mut self) -> Option<Self::Item> {
        let cur = self.cursor.take()?;

        match cur.as_ref() {
            Atom::Nil => None,
            Atom::Cons(cell) => {
                self.cursor = Some(cell.cdr.clone());
                Some(cell.car.clone())
            }
            Atom::Fun(_fun) => {
                self.cursor = None;
                todo!()
            }
            _ => {
                self.cursor = None;
                Some(cur)
            }
        }
    }
}
