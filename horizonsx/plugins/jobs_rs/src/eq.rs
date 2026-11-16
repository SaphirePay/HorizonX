//! Jobs' equations (`10*(joblevel)+(joblevel*joblevel*4)`): parsed once into a tree, evaluated with the
//! variables of the moment (joblevel, numjobs, maxjobs, baseincome, basepoints, baseexperience, totallevel...).

#[derive(Clone, Debug)]
pub enum Expr {
    Num(f64),
    Var(String),
    Neg(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

pub fn parse(text: &str) -> Expr {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    let mut p = Parser { s: chars, i: 0 };
    let e = p.sum();
    if p.i < p.s.len() { Expr::Num(0.0) } else { e }
}

struct Parser {
    s: Vec<char>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn sum(&mut self) -> Expr {
        let mut left = self.product();
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.i += 1;
            let right = self.product();
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        left
    }
    fn product(&mut self) -> Expr {
        let mut left = self.power();
        while let Some(op @ ('*' | '/' | '%')) = self.peek() {
            self.i += 1;
            let right = self.power();
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        left
    }
    fn power(&mut self) -> Expr {
        let base = self.unary();
        if self.peek() == Some('^') {
            self.i += 1;
            let exp = self.power();
            return Expr::Bin('^', Box::new(base), Box::new(exp));
        }
        base
    }
    fn unary(&mut self) -> Expr {
        match self.peek() {
            Some('-') => {
                self.i += 1;
                Expr::Neg(Box::new(self.unary()))
            }
            Some('+') => {
                self.i += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }
    fn atom(&mut self) -> Expr {
        match self.peek() {
            Some('(') => {
                self.i += 1;
                let e = self.sum();
                if self.peek() == Some(')') {
                    self.i += 1;
                }
                e
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let start = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
                    self.i += 1;
                }
                let t: String = self.s[start..self.i].iter().collect();
                Expr::Num(t.parse().unwrap_or(0.0))
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                let start = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                    self.i += 1;
                }
                let name: String = self.s[start..self.i].iter().collect::<String>().to_ascii_lowercase();
                if self.peek() == Some('(') {
                    self.i += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(')') {
                        loop {
                            args.push(self.sum());
                            if self.peek() == Some(',') {
                                self.i += 1;
                                continue;
                            }
                            break;
                        }
                    }
                    if self.peek() == Some(')') {
                        self.i += 1;
                    }
                    return Expr::Call(name, args);
                }
                Expr::Var(name)
            }
            _ => {
                self.i = self.s.len() + 1;
                Expr::Num(0.0)
            }
        }
    }
}

impl Expr {
    /// The value with `vars` (name, value) (an unknown variable is 0).
    pub fn eval(&self, vars: &[(&str, f64)]) -> f64 {
        match self {
            Expr::Num(n) => *n,
            Expr::Var(v) => vars.iter().find(|(k, _)| *k == v).map(|(_, x)| *x).unwrap_or(0.0),
            Expr::Neg(e) => -e.eval(vars),
            Expr::Bin(op, a, b) => {
                let (a, b) = (a.eval(vars), b.eval(vars));
                match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => {
                        if b == 0.0 {
                            0.0
                        } else {
                            a / b
                        }
                    }
                    '%' => {
                        if b == 0.0 {
                            0.0
                        } else {
                            a % b
                        }
                    }
                    _ => a.powf(b),
                }
            }
            Expr::Call(f, args) => {
                let a = |i: usize| args.get(i).map(|e| e.eval(vars)).unwrap_or(0.0);
                match f.as_str() {
                    "sqrt" => a(0).sqrt(),
                    "abs" => a(0).abs(),
                    "floor" => a(0).floor(),
                    "ceil" => a(0).ceil(),
                    "round" => a(0).round(),
                    "min" => a(0).min(a(1)),
                    "max" => a(0).max(a(1)),
                    "pow" => a(0).powf(a(1)),
                    "log" | "ln" => a(0).ln(),
                    "log10" => a(0).log10(),
                    "exp" => a(0).exp(),
                    _ => 0.0,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn equations() {
        let e = super::parse("10*(joblevel)+(joblevel*joblevel*4)");
        assert_eq!(e.eval(&[("joblevel", 2.0)]), 36.0);
        let e = super::parse("baseincome+(baseincome*(joblevel-1)*0.01)-((baseincome+(baseincome*(joblevel-1)*0.01)) * ((numjobs-1)*0.05))");
        assert!((e.eval(&[("baseincome", 1.0), ("joblevel", 1.0), ("numjobs", 1.0)]) - 1.0).abs() < 1e-9);
        assert_eq!(super::parse("2^3^2").eval(&[]), 512.0);
        assert_eq!(super::parse("-3+5").eval(&[]), 2.0);
    }
}
