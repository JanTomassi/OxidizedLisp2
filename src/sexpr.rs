use std::{
    fmt::{self, Debug, Formatter},
    ops::Index,
    sync::{Arc, OnceLock},
};

use crate::atom::{Atom, SAtom};

#[derive(PartialEq, Clone)]
pub struct SExpr {
    pub car: SAtom,
    pub cdr: SAtom,
    pub len: usize,
}

impl SExpr {
    pub fn get(&self, index: usize) -> Option<&Atom> {
        let mut it = self;
        let mut idx = 0;

        loop {
            if idx == index {
                return Some(it.car.as_ref());
            }

            idx += 1;

            match it.cdr.as_ref() {
                Atom::Cons(next) => it = next,
                tail if idx == index => return Some(tail),
                _ => return None,
            }
        }
    }

    #[inline]
    pub fn iter(&self) -> SExprIter<'_> {
        SExprIter {
            cursor: Cursor::Cell(self),
        }
    }

    #[inline]
    fn nil_atom() -> SAtom {
        static NIL: OnceLock<SAtom> = OnceLock::new();
        Arc::clone(NIL.get_or_init(|| Arc::new(Atom::Nil)))
    }

    #[inline]
    pub fn empty_list() -> SAtom {
        Self::nil_atom()
    }

    #[inline]
    fn from_shared_items(items: Vec<SAtom>) -> SAtom {
        let mut rev = items.into_iter().rev();

        let Some(last) = rev.next() else {
            return Self::empty_list();
        };

        let mut list = SExpr {
            car: last,
            cdr: Self::nil_atom(),
            len: 1,
        };

        for (i, car) in rev.enumerate() {
            list = SExpr {
                car,
                cdr: Arc::new(Atom::Cons(list)),
                len: i + 2,
            };
        }

        Arc::new(Atom::Cons(list))
    }

    #[inline]
    fn collect_shared<I, T>(iter: I) -> Vec<SAtom>
    where
        I: IntoIterator<Item = T>,
        T: Into<SAtom>,
    {
        let iter = iter.into_iter();
        let (lower, upper) = iter.size_hint();

        let mut items = Vec::with_capacity(upper.unwrap_or(lower));
        items.extend(iter.map(Into::into));
        items
    }

    #[inline]
    pub fn from_satoms<I>(iter: I) -> SAtom
    where
        I: IntoIterator<Item = SAtom>,
    {
        Self::from_shared_items(Self::collect_shared(iter))
    }

    #[inline]
    pub fn from_atoms<I>(iter: I) -> SAtom
    where
        I: IntoIterator<Item = Atom>,
    {
        Self::from_shared_items(Self::collect_shared(iter))
    }
}

impl Index<usize> for SExpr {
    type Output = Atom;

    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).expect("index out of range")
    }
}

pub struct SExprIter<'a> {
    pub(crate) cursor: Cursor<'a>,
}

#[derive(Copy, Clone)]
pub(crate) enum Cursor<'a> {
    Done,
    Cell(&'a SExpr),
    Tail(&'a SAtom),
}

impl<'a> Iterator for SExprIter<'a> {
    type Item = SAtom;

    fn next(&mut self) -> Option<Self::Item> {
        match self.cursor {
            Cursor::Done => None,

            Cursor::Tail(atom) => {
                self.cursor = Cursor::Done;
                Some(atom.clone())
            }

            Cursor::Cell(cell) => {
                let out = cell.car.clone();

                self.cursor = match cell.cdr.as_ref() {
                    Atom::Nil => Cursor::Done,
                    Atom::Cons(next) => Cursor::Cell(next),
                    _ => Cursor::Tail(&cell.cdr),
                };

                Some(out)
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self.cursor {
            Cursor::Done => (0, Some(0)),
            Cursor::Cell(cell) => (cell.len, Some(cell.len)),
            Cursor::Tail(_) => (1, Some(1)),
        }
    }
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
