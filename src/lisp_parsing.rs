use crate::{
    atom::{Atom, SAtom},
    sexpr::SExpr,
};

use nom::{
    self,
    branch::alt,
    bytes::complete::{is_not, tag},
    character::complete::{alpha1, alphanumeric1, char, multispace0},
    error::ParseError,
    multi::{fold_many0, many0},
    number::complete::double,
    sequence::delimited,
    IResult, Parser,
};

fn ws<'a, O, E, F>(inner: F) -> impl Parser<&'a str, Output = O, Error = E>
where
    E: ParseError<&'a str>,
    F: Parser<&'a str, Output = O, Error = E>,
{
    delimited(multispace0, inner, multispace0)
}

fn parse_num(input: &str) -> IResult<&str, Atom> {
    let res: (&str, f64) = double(input)?;
    Ok((res.0, Atom::Num(res.1)))
}

fn parse_str(input: &str) -> IResult<&str, Atom> {
    let res = delimited(char('"'), is_not("\""), char('"')).parse(input)?;
    Ok((res.0, Atom::Str(res.1.to_string())))
}

fn parse_sym(input: &str) -> IResult<&str, Atom> {
    let res = (
        alpha1,
        fold_many0(
            alt((alphanumeric1, tag("_"))),
            String::default,
            |mut acc: String, item| {
                acc += item;
                acc
            },
        ),
    )
        .parse(input)?;
    Ok((res.0, Atom::Sym(res.1 .0.to_string() + &res.1 .1)))
}

fn parse_sexp(input: &str) -> IResult<&str, Atom> {
    let p = delimited(char('('), many0(parse_atom), char(')')).parse(input)?;
    let args = p.1;
    let sexpr = args
        .into_iter()
        .rev()
        .fold(Atom::default(), |tail, item: Atom| {
            Atom::Cons(SExpr {
                car: SAtom::new(item),
                cdr: SAtom::new(tail),
            })
        });

    Ok((p.0, sexpr))
}

pub fn parse_atom(input: &str) -> IResult<&str, Atom> {
    alt((ws(parse_num), ws(parse_str), ws(parse_sym), ws(parse_sexp))).parse(input)
}

pub fn parse(input: &str) -> Atom {
    let res = parse_atom(input).unwrap();
    res.1
}

#[cfg(test)]
mod tests {
    use crate::{nil, num, sexpr, str, sym};

    use super::*;

    #[test]
    fn test_simple_val() {
        assert_eq!(parse_atom("0").unwrap().1, num!(0));
        assert_eq!(parse_atom("\"Testing\"").unwrap().1, str!("Testing"));
        assert_eq!(
            parse_atom("  Symbol_Second  ").unwrap().1,
            sym!("Symbol_Second")
        );
    }

    #[test]
    fn test_sexp() {
        assert_eq!(parse_atom("()").unwrap().1, nil!());
        assert_eq!(parse_atom("(sym)").unwrap().1, sexpr!(sym!("sym")));
        assert_eq!(
            parse_atom("(add 1 2)").unwrap().1,
            sexpr!(sym!("add"), num!(1), num!(2),)
        );
    }
}
