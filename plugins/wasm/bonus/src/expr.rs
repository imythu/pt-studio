//! 安全表达式语言（bonus 03 §1.1）：只有 f64 一种值；无赋值、无循环、无用户函数；函数白名单。
//! 解析为扁平节点数组（下标引用）；解析深度上限 32、节点上限 512，求值递归深度因此有界。

use std::collections::BTreeMap;

pub const MAX_LEN: usize = 4000;
pub const MAX_DEPTH: usize = 32;
pub const MAX_NODES: usize = 512;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Str(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
    Question,
    Colon,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub code: &'static str,
    pub pos: usize,
    pub message: String,
}

fn issue(code: &'static str, pos: usize, message: impl Into<String>) -> Issue {
    Issue {
        code,
        pos,
        message: message.into(),
    }
}

fn lex(src: &str) -> Result<Vec<(Tok, usize)>, Issue> {
    if src.len() > MAX_LEN {
        return Err(issue(
            "bonus.expression_too_complex",
            0,
            "expression is longer than 4000 characters",
        ));
    }
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if !c.is_ascii() || (c.is_ascii_control() && !c.is_ascii_whitespace()) || c == b';' || c == b'`' {
            return Err(issue("bonus.expression_syntax", i, "forbidden character"));
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                i += 1;
                if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
                    i += 1;
                }
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let v: f64 = src[start..i]
                .parse()
                .map_err(|_| issue("bonus.expression_syntax", start, "bad number"))?;
            out.push((Tok::Num(v), start));
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let id = &src[start..i];
            if id.len() > 32 {
                return Err(issue(
                    "bonus.expression_syntax",
                    start,
                    "identifier longer than 32 characters",
                ));
            }
            let t = match id {
                "and" => Tok::Op("and"),
                "or" => Tok::Op("or"),
                "not" => Tok::Op("not"),
                _ => Tok::Ident(id.to_owned()),
            };
            out.push((t, start));
            continue;
        }
        if c == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' {
                if !(b[i].is_ascii_alphanumeric() || b"_-:.".contains(&b[i])) {
                    return Err(issue(
                        "bonus.expression_syntax",
                        i,
                        "invalid character in string literal",
                    ));
                }
                i += 1;
            }
            if i >= b.len() || i - start - 1 > 64 {
                return Err(issue(
                    "bonus.expression_syntax",
                    start,
                    "unterminated or too long string literal",
                ));
            }
            out.push((Tok::Str(src[start + 1..i].to_owned()), start));
            i += 1;
            continue;
        }
        let two = src.get(i..i + 2).unwrap_or("");
        let op2 = ["==", "!=", "<=", ">=", "&&", "||"].iter().find(|o| **o == two);
        if let Some(o) = op2 {
            let o: &'static str = match *o {
                "&&" => "and",
                "||" => "or",
                x => x,
            };
            out.push((Tok::Op(o), start));
            i += 2;
            continue;
        }
        let t = match c {
            b'+' => Tok::Op("+"),
            b'-' => Tok::Op("-"),
            b'*' => Tok::Op("*"),
            b'/' => Tok::Op("/"),
            b'%' => Tok::Op("%"),
            b'^' => Tok::Op("^"),
            b'<' => Tok::Op("<"),
            b'>' => Tok::Op(">"),
            b'!' => Tok::Op("not"),
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
            b',' => Tok::Comma,
            b'?' => Tok::Question,
            b':' => Tok::Colon,
            _ => return Err(issue("bonus.expression_syntax", i, "unexpected character")),
        };
        out.push((t, start));
        i += 1;
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    Min,
    Max,
    Clamp,
    Abs,
    Floor,
    Ceil,
    Round,
    Trunc,
    Sqrt,
    Ln,
    Log10,
    Log2,
    Exp,
    Pow,
    Atan,
    Tanh,
    If,
    SafeDiv,
    Between,
    Tag,
    Type,
    HasRole,
}

fn func_of(name: &str) -> Option<(Func, usize, usize)> {
    // (函数, 最少参数, 最多参数)
    Some(match name {
        "min" => (Func::Min, 2, 16),
        "max" => (Func::Max, 2, 16),
        "clamp" => (Func::Clamp, 3, 3),
        "abs" => (Func::Abs, 1, 1),
        "floor" => (Func::Floor, 1, 1),
        "ceil" => (Func::Ceil, 1, 1),
        "round" => (Func::Round, 1, 1),
        "trunc" => (Func::Trunc, 1, 1),
        "sqrt" => (Func::Sqrt, 1, 1),
        "ln" => (Func::Ln, 1, 1),
        "log10" => (Func::Log10, 1, 1),
        "log2" => (Func::Log2, 1, 1),
        "exp" => (Func::Exp, 1, 1),
        "pow" => (Func::Pow, 2, 2),
        "atan" => (Func::Atan, 1, 1),
        "tanh" => (Func::Tanh, 1, 1),
        "if" => (Func::If, 3, 3),
        "safe_div" => (Func::SafeDiv, 3, 3),
        "between" => (Func::Between, 3, 3),
        "tag" => (Func::Tag, 1, 1),
        "type" => (Func::Type, 1, 1),
        "has_role" => (Func::HasRole, 1, 1),
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Num(f64),
    Var(usize),
    Neg(usize),
    Not(usize),
    Bin(&'static str, usize, usize),
    And(usize, usize),
    Or(usize, usize),
    Cond(usize, usize, usize),
    Call(Func, Vec<usize>),
    StrCall(Func, String),
}

/// 变量表：名字 → 下标；编译时解析，求值时按下标取值。
#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub names: Vec<String>,
}

impl Scope {
    pub fn new(names: &[&str]) -> Self {
        Self {
            names: names.iter().map(|s| (*s).to_owned()).collect(),
        }
    }
    pub fn add(&mut self, name: &str) -> usize {
        if let Some(i) = self.index(name) {
            return i;
        }
        self.names.push(name.to_owned());
        self.names.len() - 1
    }
    pub fn index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }
}

/// 编译期依赖（只获取需要的数据）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Deps {
    pub vars: Vec<usize>,
    pub needs_roles: bool,
    pub needs_tags: bool,
}

#[derive(Debug, Clone)]
pub struct Compiled {
    nodes: Vec<Node>,
    root: usize,
    pub deps: Deps,
}

/// 求值环境：变量值、标签、类型、角色
pub trait Env {
    fn var(&self, i: usize) -> f64;
    fn has_tag(&self, _t: &str) -> bool {
        false
    }
    fn is_type(&self, _t: &str) -> bool {
        false
    }
    fn has_role(&self, _r: &str) -> bool {
        false
    }
}

fn constant(name: &str) -> Option<f64> {
    Some(match name {
        "pi" => std::f64::consts::PI,
        "e" => std::f64::consts::E,
        "KiB" => 1024.0,
        "MiB" => 1_048_576.0,
        "GiB" => 1_073_741_824.0,
        "TiB" => 1_099_511_627_776.0,
        "HOUR" => 3600.0,
        "DAY" => 86_400.0,
        "WEEK" => 604_800.0,
        "YEAR" => 31_536_000.0,
        _ => return None,
    })
}

struct Parser<'a> {
    toks: Vec<(Tok, usize)>,
    pos: usize,
    nodes: Vec<Node>,
    scope: &'a Scope,
    params: &'a BTreeMap<String, f64>,
    deps: Deps,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.0)
    }
    fn at(&self) -> usize {
        self.toks.get(self.pos).map_or(usize::MAX, |t| t.1)
    }
    fn push(&mut self, n: Node) -> Result<usize, Issue> {
        if self.nodes.len() >= MAX_NODES {
            return Err(issue(
                "bonus.expression_too_complex",
                self.at(),
                "expression has too many nodes",
            ));
        }
        self.nodes.push(n);
        Ok(self.nodes.len() - 1)
    }
    fn enter(&mut self) -> Result<(), Issue> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(issue(
                "bonus.expression_too_complex",
                self.at(),
                "expression nests too deeply",
            ));
        }
        Ok(())
    }
    fn eat_op(&mut self, op: &str) -> bool {
        if self.peek()
            == Some(&Tok::Op(match op {
                "+" => "+",
                "-" => "-",
                "*" => "*",
                "/" => "/",
                "%" => "%",
                "^" => "^",
                "and" => "and",
                "or" => "or",
                "not" => "not",
                "==" => "==",
                "!=" => "!=",
                "<" => "<",
                "<=" => "<=",
                ">" => ">",
                ">=" => ">=",
                _ => return false,
            }))
        {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self) -> Result<usize, Issue> {
        self.enter()?;
        let c = self.or_expr()?;
        let r = if self.peek() == Some(&Tok::Question) {
            self.pos += 1;
            let a = self.expr()?;
            if self.peek() != Some(&Tok::Colon) {
                return Err(issue("bonus.expression_syntax", self.at(), "expected `:`"));
            }
            self.pos += 1;
            let b = self.expr()?;
            self.push(Node::Cond(c, a, b))?
        } else {
            c
        };
        self.depth -= 1;
        Ok(r)
    }
    fn or_expr(&mut self) -> Result<usize, Issue> {
        let mut l = self.and_expr()?;
        while self.eat_op("or") {
            let r = self.and_expr()?;
            l = self.push(Node::Or(l, r))?;
        }
        Ok(l)
    }
    fn and_expr(&mut self) -> Result<usize, Issue> {
        let mut l = self.not_expr()?;
        while self.eat_op("and") {
            let r = self.not_expr()?;
            l = self.push(Node::And(l, r))?;
        }
        Ok(l)
    }
    fn not_expr(&mut self) -> Result<usize, Issue> {
        if self.eat_op("not") {
            self.enter()?;
            let x = self.not_expr()?;
            self.depth -= 1;
            return self.push(Node::Not(x));
        }
        self.cmp()
    }
    fn cmp(&mut self) -> Result<usize, Issue> {
        let l = self.add()?;
        for op in ["==", "!=", "<=", ">=", "<", ">"] {
            if self.eat_op(op) {
                let r = self.add()?;
                let op: &'static str = match op {
                    "==" => "==",
                    "!=" => "!=",
                    "<=" => "<=",
                    ">=" => ">=",
                    "<" => "<",
                    _ => ">",
                };
                if matches!(self.peek(), Some(Tok::Op("==" | "!=" | "<" | "<=" | ">" | ">="))) {
                    return Err(issue(
                        "bonus.expression_syntax",
                        self.at(),
                        "chained comparisons are not allowed",
                    ));
                }
                return self.push(Node::Bin(op, l, r));
            }
        }
        Ok(l)
    }
    fn add(&mut self) -> Result<usize, Issue> {
        let mut l = self.mul()?;
        loop {
            let op = if self.eat_op("+") {
                "+"
            } else if self.eat_op("-") {
                "-"
            } else {
                return Ok(l);
            };
            let r = self.mul()?;
            l = self.push(Node::Bin(op, l, r))?;
        }
    }
    fn mul(&mut self) -> Result<usize, Issue> {
        let mut l = self.unary()?;
        loop {
            let op = if self.eat_op("*") {
                "*"
            } else if self.eat_op("/") {
                "/"
            } else if self.eat_op("%") {
                "%"
            } else {
                return Ok(l);
            };
            let r = self.unary()?;
            l = self.push(Node::Bin(op, l, r))?;
        }
    }
    fn unary(&mut self) -> Result<usize, Issue> {
        if self.eat_op("-") {
            self.enter()?;
            let x = self.unary()?;
            self.depth -= 1;
            return self.push(Node::Neg(x));
        }
        self.power()
    }
    fn power(&mut self) -> Result<usize, Issue> {
        let base = self.primary()?;
        if self.eat_op("^") {
            self.enter()?;
            let e = self.unary()?; // 右结合
            self.depth -= 1;
            return self.push(Node::Bin("^", base, e));
        }
        Ok(base)
    }
    fn primary(&mut self) -> Result<usize, Issue> {
        let pos = self.at();
        let Some(t) = self.toks.get(self.pos).map(|t| t.0.clone()) else {
            return Err(issue(
                "bonus.expression_syntax",
                pos,
                "unexpected end of expression",
            ));
        };
        self.pos += 1;
        match t {
            Tok::Num(v) => self.push(Node::Num(v)),
            Tok::LParen => {
                let e = self.expr()?;
                if self.peek() != Some(&Tok::RParen) {
                    return Err(issue("bonus.expression_syntax", self.at(), "expected `)`"));
                }
                self.pos += 1;
                Ok(e)
            }
            Tok::Ident(name) if self.peek() == Some(&Tok::LParen) => {
                self.pos += 1;
                let (f, lo, hi) = func_of(&name).ok_or_else(|| {
                    issue(
                        "bonus.expression_unknown_function",
                        pos,
                        format!("unknown function `{name}`"),
                    )
                })?;
                if matches!(f, Func::Tag | Func::Type | Func::HasRole) {
                    let Some((Tok::Str(s), _)) = self.toks.get(self.pos).cloned() else {
                        return Err(issue(
                            "bonus.expression_bad_arguments",
                            pos,
                            format!("`{name}` takes one string literal"),
                        ));
                    };
                    self.pos += 1;
                    if self.peek() != Some(&Tok::RParen) {
                        return Err(issue(
                            "bonus.expression_bad_arguments",
                            pos,
                            format!("`{name}` takes one string literal"),
                        ));
                    }
                    self.pos += 1;
                    match f {
                        Func::HasRole => self.deps.needs_roles = true,
                        _ => self.deps.needs_tags = true,
                    }
                    return self.push(Node::StrCall(f, s));
                }
                let mut args = Vec::new();
                if self.peek() != Some(&Tok::RParen) {
                    loop {
                        if matches!(self.peek(), Some(Tok::Str(_))) {
                            return Err(issue(
                                "bonus.expression_bad_arguments",
                                self.at(),
                                "string literals are only allowed in tag/type/has_role",
                            ));
                        }
                        args.push(self.expr()?);
                        if self.peek() == Some(&Tok::Comma) {
                            self.pos += 1;
                            continue;
                        }
                        break;
                    }
                }
                if self.peek() != Some(&Tok::RParen) {
                    return Err(issue("bonus.expression_syntax", self.at(), "expected `)`"));
                }
                self.pos += 1;
                if args.len() < lo || args.len() > hi {
                    return Err(issue(
                        "bonus.expression_bad_arguments",
                        pos,
                        format!("`{name}` takes {lo}..={hi} arguments"),
                    ));
                }
                self.push(Node::Call(f, args))
            }
            Tok::Ident(name) => {
                if let Some(v) = self.params.get(&name).copied().or_else(|| constant(&name)) {
                    return self.push(Node::Num(v));
                }
                let i = self.scope.index(&name).ok_or_else(|| {
                    issue(
                        "bonus.expression_unknown_identifier",
                        pos,
                        format!("unknown identifier `{name}`"),
                    )
                })?;
                if !self.deps.vars.contains(&i) {
                    self.deps.vars.push(i);
                }
                self.push(Node::Var(i))
            }
            Tok::Str(_) => Err(issue(
                "bonus.expression_bad_arguments",
                pos,
                "string literal outside tag/type/has_role",
            )),
            _ => Err(issue("bonus.expression_syntax", pos, "unexpected token")),
        }
    }
}

/// 编译：`params` 中的参数（`^[A-Z][A-Z0-9_]{0,31}$`）与常量在编译期替换为数值。
pub fn compile(src: &str, scope: &Scope, params: &BTreeMap<String, f64>) -> Result<Compiled, Issue> {
    let toks = lex(src)?;
    if toks.is_empty() {
        return Err(issue("bonus.expression_syntax", 0, "empty expression"));
    }
    let mut p = Parser {
        toks,
        pos: 0,
        nodes: Vec::new(),
        scope,
        params,
        deps: Deps::default(),
        depth: 0,
    };
    let root = p.expr()?;
    if p.pos != p.toks.len() {
        return Err(issue(
            "bonus.expression_syntax",
            p.at(),
            "unexpected trailing input",
        ));
    }
    Ok(Compiled {
        nodes: p.nodes,
        root,
        deps: p.deps,
    })
}

fn b(x: bool) -> f64 {
    if x { 1.0 } else { 0.0 }
}

/// 求值错误（如 `exp` 参数过大）：调用方把结果记为 0 并置标志
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalError;

impl Compiled {
    /// 求值；非有限结果返回 `Ok(0.0)` 并置 `nonfinite`。
    pub fn eval(&self, env: &dyn Env, nonfinite: &mut bool) -> Result<f64, EvalError> {
        let v = self.node(self.root, env)?;
        if v.is_finite() {
            Ok(v)
        } else {
            *nonfinite = true;
            Ok(0.0)
        }
    }

    fn node(&self, i: usize, env: &dyn Env) -> Result<f64, EvalError> {
        let n = |j: usize| self.node(j, env);
        Ok(match &self.nodes[i] {
            Node::Num(v) => *v,
            Node::Var(k) => env.var(*k),
            Node::Neg(x) => -n(*x)?,
            Node::Not(x) => b(n(*x)? == 0.0),
            Node::And(a, c) => b(n(*a)? != 0.0 && n(*c)? != 0.0),
            Node::Or(a, c) => b(n(*a)? != 0.0 || n(*c)? != 0.0),
            Node::Cond(c, a, d) => {
                if n(*c)? != 0.0 {
                    n(*a)?
                } else {
                    n(*d)?
                }
            }
            Node::Bin(op, a, c) => {
                let (x, y) = (n(*a)?, n(*c)?);
                match *op {
                    "+" => x + y,
                    "-" => x - y,
                    "*" => x * y,
                    "/" => {
                        if y == 0.0 {
                            0.0
                        } else {
                            x / y
                        }
                    }
                    "%" => {
                        if y == 0.0 {
                            0.0
                        } else {
                            x % y
                        }
                    }
                    "^" => {
                        if y.abs() > 64.0 {
                            return Err(EvalError);
                        }
                        x.powf(y)
                    }
                    "==" => b(x == y),
                    "!=" => b(x != y),
                    "<" => b(x < y),
                    "<=" => b(x <= y),
                    ">" => b(x > y),
                    _ => b(x >= y),
                }
            }
            Node::StrCall(f, s) => b(match f {
                Func::Tag => env.has_tag(s),
                Func::Type => env.is_type(s),
                _ => env.has_role(s),
            }),
            Node::Call(f, args) => {
                // if 只求值被选中的分支
                if *f == Func::If {
                    return if n(args[0])? != 0.0 {
                        n(args[1])
                    } else {
                        n(args[2])
                    };
                }
                let v: Vec<f64> = args.iter().map(|a| n(*a)).collect::<Result<_, _>>()?;
                match f {
                    Func::Min => v.iter().copied().fold(f64::INFINITY, f64::min),
                    Func::Max => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    Func::Clamp => v[0].max(v[1]).min(v[2]),
                    Func::Abs => v[0].abs(),
                    Func::Floor => v[0].floor(),
                    Func::Ceil => v[0].ceil(),
                    Func::Round => v[0].round(),
                    Func::Trunc => v[0].trunc(),
                    Func::Sqrt => {
                        if v[0] < 0.0 {
                            0.0
                        } else {
                            v[0].sqrt()
                        }
                    }
                    Func::Ln | Func::Log10 | Func::Log2 if v[0] <= 0.0 => 0.0,
                    Func::Ln => v[0].ln(),
                    Func::Log10 => v[0].log10(),
                    Func::Log2 => v[0].log2(),
                    Func::Exp => {
                        if v[0] > 700.0 {
                            return Err(EvalError);
                        }
                        v[0].exp()
                    }
                    Func::Pow => {
                        if v[1].abs() > 64.0 {
                            return Err(EvalError);
                        }
                        v[0].powf(v[1])
                    }
                    Func::Atan => v[0].atan(),
                    Func::Tanh => v[0].tanh(),
                    Func::SafeDiv => {
                        if v[1] == 0.0 {
                            v[2]
                        } else {
                            v[0] / v[1]
                        }
                    }
                    Func::Between => b(v[1] <= v[0] && v[0] < v[2]),
                    Func::If | Func::Tag | Func::Type | Func::HasRole => 0.0,
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct E(Vec<f64>);
    impl Env for E {
        fn var(&self, i: usize) -> f64 {
            self.0[i]
        }
        fn has_tag(&self, t: &str) -> bool {
            t == "official"
        }
    }

    fn ev(src: &str, vars: &[(&str, f64)]) -> Result<f64, String> {
        let names: Vec<&str> = vars.iter().map(|v| v.0).collect();
        let scope = Scope::new(&names);
        let params: BTreeMap<String, f64> = [("T0".to_owned(), 4.0)].into();
        let c = compile(src, &scope, &params).map_err(|e| format!("{}: {}", e.code, e.message))?;
        let mut nf = false;
        c.eval(&E(vars.iter().map(|v| v.1).collect()), &mut nf)
            .map_err(|_| "eval".into())
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(ev("1 + 2 * 3", &[]), Ok(7.0));
        assert_eq!(ev("2 ^ 3 ^ 2", &[]), Ok(512.0)); // 右结合
        assert_eq!(ev("-2 ^ 2", &[]), Ok(-4.0));
        assert_eq!(ev("(1 + 2) * 3", &[]), Ok(9.0));
        assert_eq!(ev("7 % 3", &[]), Ok(1.0));
        assert_eq!(ev("1 / 0", &[]), Ok(0.0));
        assert_eq!(ev("gib * T0", &[("gib", 2.5)]), Ok(10.0));
        assert_eq!(ev("GiB / MiB", &[]), Ok(1024.0));
    }

    #[test]
    fn logic_and_functions() {
        assert_eq!(ev("1 < 2 and not 0", &[]), Ok(1.0));
        assert_eq!(ev("0 || 0", &[]), Ok(0.0));
        assert_eq!(ev("x > 1 ? 10 : 20", &[("x", 2.0)]), Ok(10.0));
        assert_eq!(ev("if(tag(\"official\"), 3, 4)", &[]), Ok(3.0));
        assert_eq!(ev("if(tag(\"zero\"), 3, 4)", &[]), Ok(4.0));
        assert_eq!(ev("max(1, 5, 3) + min(2, 9)", &[]), Ok(7.0));
        assert_eq!(ev("clamp(15, 0, 10)", &[]), Ok(10.0));
        assert_eq!(ev("safe_div(1, 0, 42)", &[]), Ok(42.0));
        assert_eq!(ev("between(5, 1, 5)", &[]), Ok(0.0));
        assert_eq!(ev("sqrt(-4) + ln(0)", &[]), Ok(0.0));
        assert!(ev("exp(701)", &[]).is_err());
        // if 只求值选中分支：未选中分支中的溢出不报错
        assert_eq!(ev("if(1, 2, exp(701))", &[]), Ok(2.0));
    }

    #[test]
    fn rejects_bad_input() {
        for (src, code) in [
            ("1 +", "bonus.expression_syntax"),
            ("1 < 2 < 3", "bonus.expression_syntax"),
            ("foo + 1", "bonus.expression_unknown_identifier"),
            ("system(1)", "bonus.expression_unknown_function"),
            ("min(1)", "bonus.expression_bad_arguments"),
            ("tag(1)", "bonus.expression_bad_arguments"),
            ("1; 2", "bonus.expression_syntax"),
            ("\"x\" + 1", "bonus.expression_bad_arguments"),
        ] {
            let r = ev(src, &[]);
            assert!(r.as_ref().is_err_and(|e| e.starts_with(code)), "{src}: {r:?}");
        }
        let deep = format!("{}1{}", "(".repeat(40), ")".repeat(40));
        assert!(ev(&deep, &[]).is_err_and(|e| e.starts_with("bonus.expression_too_complex")));
        let wide = vec!["1"; 600].join(" + ");
        assert!(ev(&wide, &[]).is_err_and(|e| e.starts_with("bonus.expression_too_complex")));
    }

    #[test]
    fn nexusphp_shape() {
        // NexusPHP 形状的 A 项：1 GiB、4 周、单人做种
        let a = ev(
            "(1 - exp(ln(0.1) / T0 * age_weeks)) * gib * (1 + sqrt(2) * exp(ln(0.1) * (seeders - 1) / (7 - 1)))",
            &[("age_weeks", 4.0), ("gib", 1.0), ("seeders", 1.0)],
        )
        .unwrap_or(-1.0);
        assert!((a - 0.9 * (1.0 + 2f64.sqrt())).abs() < 1e-9, "{a}");
    }
}
