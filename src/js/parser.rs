//! Recursive-descent parser for the built-in JavaScript subset interpreter.
//!
//! Produces a `Vec<Stmt>` AST. Supports a pragmatic subset: variable
//! declarations, assignment, if/else, for, for-of, while, function
//! declarations, return/break/continue, throw, blocks, and a full expression
//! grammar with precedence climbing.

use super::ast::*;
use super::lexer::{tokenize, Token, TmplTokenPart};

#[derive(Debug, Clone)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "parse error: {}", self.0)
    }
}
impl std::error::Error for ParseError {}

type PResult<T> = Result<T, ParseError>;

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(src: &str) -> PResult<Self> {
        let toks = tokenize(src).map_err(|e| ParseError(e.to_string()))?;
        Ok(Self { toks, pos: 0 })
    }

    pub fn parse_program(&mut self) -> PResult<Vec<Stmt>> {
        let mut out = Vec::new();
        while !self.is_eof() {
            // Allow stray semicolons at top level.
            if self.peek_punct(";") {
                self.advance();
                continue;
            }
            out.push(self.parse_stmt()?);
        }
        Ok(out)
    }

    fn is_eof(&self) -> bool {
        matches!(self.toks.get(self.pos), Some(Token::Eof) | None)
    }

    fn peek(&self) -> &Token {
        self.toks.get(self.pos).unwrap_or(&Token::Eof)
    }

    #[allow(dead_code)]
    fn peek_n(&self, n: usize) -> &Token {
        self.toks.get(self.pos + n).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let t = self.toks.get(self.pos).cloned().unwrap_or(Token::Eof);
        if !matches!(t, Token::Eof) {
            self.pos += 1;
        }
        t
    }

    fn peek_punct(&self, p: &str) -> bool {
        matches!(self.peek(), Token::Punct(x) if *x == p)
    }

    fn eat_punct(&mut self, p: &str) -> bool {
        if self.peek_punct(p) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect_punct(&mut self, p: &str) -> PResult<()> {
        if self.eat_punct(p) {
            Ok(())
        } else {
            Err(ParseError(format!("expected `{}`, got {:?}", p, self.peek())))
        }
    }

    fn peek_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Token::Keyword(s) if s == k)
    }

    fn eat_kw(&mut self, k: &str) -> bool {
        if self.peek_kw(k) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect_kw(&mut self, k: &str) -> PResult<()> {
        if self.eat_kw(k) {
            Ok(())
        } else {
            Err(ParseError(format!("expected `{}`, got {:?}", k, self.peek())))
        }
    }

    fn parse_stmt(&mut self) -> PResult<Stmt> {
        match self.peek() {
            Token::Keyword(k) => match k.as_str() {
                "var" | "let" | "const" => self.parse_var(),
                "if" => self.parse_if(),
                "for" => self.parse_for(),
                "while" => self.parse_while(),
                "function" => self.parse_func_decl(),
                "return" => self.parse_return(),
                "break" => {
                    self.advance();
                    self.eat_punct(";");
                    Ok(Stmt::Break)
                }
                "continue" => {
                    self.advance();
                    self.eat_punct(";");
                    Ok(Stmt::Continue)
                }
                "throw" => {
                    self.advance();
                    let e = self.parse_expr()?;
                    self.eat_punct(";");
                    Ok(Stmt::Throw(e))
                }
                _ => self.parse_expr_or_labeled(),
            },
            Token::Punct("{") => {
                self.advance();
                let body = self.parse_block_body()?;
                Ok(Stmt::Block(body))
            }
            Token::Punct(";") => {
                self.advance();
                Ok(Stmt::Empty)
            }
            _ => self.parse_expr_stmt(),
        }
    }

    fn parse_expr_or_labeled(&mut self) -> PResult<Stmt> {
        // No labeled-loop support; fall through to expression statement.
        self.parse_expr_stmt()
    }

    fn parse_expr_stmt(&mut self) -> PResult<Stmt> {
        let e = self.parse_expr()?;
        self.eat_punct(";");
        Ok(Stmt::Expr(e))
    }

    fn parse_block_body(&mut self) -> PResult<Vec<Stmt>> {
        let mut out = Vec::new();
        while !self.peek_punct("}") && !self.is_eof() {
            out.push(self.parse_stmt()?);
        }
        self.expect_punct("}")?;
        Ok(out)
    }

    fn parse_var(&mut self) -> PResult<Stmt> {
        let kind = match self.advance() {
            Token::Keyword(k) => match k.as_str() {
                "var" => VarKind::Var,
                "let" => VarKind::Let,
                "const" => VarKind::Const,
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };
        let mut decls = Vec::new();
        loop {
            let name = self.parse_ident()?;
            let init = if self.eat_punct("=") {
                Some(self.parse_assign()?)
            } else {
                None
            };
            decls.push((name, init));
            if !self.eat_punct(",") {
                break;
            }
        }
        self.eat_punct(";");
        Ok(Stmt::Var { kind, decls })
    }

    fn parse_ident(&mut self) -> PResult<String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            other => Err(ParseError(format!("expected identifier, got {:?}", other))),
        }
    }

    fn parse_if(&mut self) -> PResult<Stmt> {
        self.expect_kw("if")?;
        self.expect_punct("(")?;
        let cond = self.parse_expr()?;
        self.expect_punct(")")?;
        let then = Box::new(self.parse_stmt()?);
        let else_ = if self.eat_kw("else") {
            Some(Box::new(self.parse_stmt()?))
        } else {
            None
        };
        Ok(Stmt::If {
            cond,
            then,
            else_,
        })
    }

    fn parse_for(&mut self) -> PResult<Stmt> {
        self.expect_kw("for")?;
        self.expect_punct("(")?;
        // Distinguish for-of: for (var x of y) / for (x of y)
        let saved = self.pos;
        let is_var_decl = self.peek_kw("var") || self.peek_kw("let") || self.peek_kw("const");
        if is_var_decl {
            let _kind_kw = match self.advance() {
                Token::Keyword(k) => k,
                _ => unreachable!(),
            };
            let name = self.parse_ident()?;
            if self.eat_kw("of") || self.eat_kw("in") {
                let iter = self.parse_expr()?;
                self.expect_punct(")")?;
                let body = Box::new(self.parse_stmt()?);
                return Ok(Stmt::ForOf { name, iter, body });
            }
            // Regular for with var init: rewind by re-parsing as declaration head.
            self.pos = saved;
        } else if matches!(self.peek(), Token::Ident(_)) {
            // possible: for (x of y)
            let name = match self.advance() {
                Token::Ident(s) => s,
                _ => unreachable!(),
            };
            if self.eat_kw("of") || self.eat_kw("in") {
                let iter = self.parse_expr()?;
                self.expect_punct(")")?;
                let body = Box::new(self.parse_stmt()?);
                return Ok(Stmt::ForOf { name, iter, body });
            }
            self.pos = saved;
        }

        // Classic for ( init? ; cond? ; update? )
        let init: Option<Box<Stmt>> = if self.peek_punct(";") {
            None
        } else if self.peek_kw("var") || self.peek_kw("let") || self.peek_kw("const") {
            Some(Box::new(self.parse_var()?))
        } else {
            let e = self.parse_expr()?;
            self.eat_punct(";");
            Some(Box::new(Stmt::Expr(e)))
        };
        if !matches!(init, Some(ref b) if matches!(**b, Stmt::Var { .. })) {
            self.eat_punct(";");
        }
        let cond = if self.peek_punct(";") {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect_punct(";")?;
        let update = if self.peek_punct(")") {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect_punct(")")?;
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::For {
            init,
            cond,
            update,
            body,
        })
    }

    fn parse_while(&mut self) -> PResult<Stmt> {
        self.expect_kw("while")?;
        self.expect_punct("(")?;
        let cond = self.parse_expr()?;
        self.expect_punct(")")?;
        let body = Box::new(self.parse_stmt()?);
        Ok(Stmt::While { cond, body })
    }

    fn parse_func_decl(&mut self) -> PResult<Stmt> {
        self.expect_kw("function")?;
        let name = self.parse_ident()?;
        self.expect_punct("(")?;
        let params = self.parse_params()?;
        self.expect_punct("{")?;
        let body = self.parse_block_body()?;
        Ok(Stmt::Func { name, params, body })
    }

    fn parse_params(&mut self) -> PResult<Vec<String>> {
        let mut params = Vec::new();
        if !self.peek_punct(")") {
            loop {
                if self.peek_punct(")") {
                    break;
                }
                // allow default values and rest; we ignore them gracefully
                if self.eat_punct("...") {
                    let _ = self.parse_ident()?;
                    params.push("rest".to_string());
                } else {
                    let name = self.parse_ident()?;
                    if self.eat_punct("=") {
                        let _ = self.parse_assign()?;
                    }
                    params.push(name);
                }
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        self.expect_punct(")")?;
        Ok(params)
    }

    fn parse_return(&mut self) -> PResult<Stmt> {
        self.expect_kw("return")?;
        let val = if self.peek_punct(";") || self.peek_punct("}") || self.is_eof() {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.eat_punct(";");
        Ok(Stmt::Return(val))
    }

    // ===== Expressions =====

    fn parse_expr(&mut self) -> PResult<Expr> {
        // comma operator -> Seq
        let first = self.parse_assign()?;
        if self.peek_punct(",") {
            let mut items = vec![first];
            while self.eat_punct(",") {
                items.push(self.parse_assign()?);
            }
            Ok(Expr::Seq(items))
        } else {
            Ok(first)
        }
    }

    fn parse_assign(&mut self) -> PResult<Expr> {
        let left = self.parse_ternary()?;
        let op = match self.peek() {
            Token::Punct("=") => Some(AssignOp::Assign),
            Token::Punct("+=") => Some(AssignOp::AddAssign),
            Token::Punct("-=") => Some(AssignOp::SubAssign),
            Token::Punct("*=") => Some(AssignOp::MulAssign),
            Token::Punct("/=") => Some(AssignOp::DivAssign),
            Token::Punct("%=") => Some(AssignOp::ModAssign),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let right = self.parse_assign()?;
            Ok(Expr::Assign(Box::new(left), op, Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn parse_ternary(&mut self) -> PResult<Expr> {
        let cond = self.parse_logical_or()?;
        if self.eat_punct("?") {
            let then = self.parse_assign()?;
            self.expect_punct(":")?;
            let else_ = self.parse_assign()?;
            Ok(Expr::Ternary(
                Box::new(cond),
                Box::new(then),
                Box::new(else_),
            ))
        } else {
            Ok(cond)
        }
    }

    fn parse_logical_or(&mut self) -> PResult<Expr> {
        let mut left = self.parse_logical_and()?;
        while self.peek_punct("||") {
            self.advance();
            let right = self.parse_logical_and()?;
            left = Expr::Logical(LogOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_logical_and(&mut self) -> PResult<Expr> {
        let mut left = self.parse_equality()?;
        while self.peek_punct("&&") {
            self.advance();
            let right = self.parse_equality()?;
            left = Expr::Logical(LogOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> PResult<Expr> {
        let mut left = self.parse_relational()?;
        loop {
            let op = match self.peek() {
                Token::Punct("==") => Some(BinOp::Eq),
                Token::Punct("!=") => Some(BinOp::Ne),
                Token::Punct("===") => Some(BinOp::StrictEq),
                Token::Punct("!==") => Some(BinOp::StrictNe),
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
                let right = self.parse_relational()?;
                left = Expr::Binary(op, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_relational(&mut self) -> PResult<Expr> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                Token::Punct("<") => Some(BinOp::Lt),
                Token::Punct(">") => Some(BinOp::Gt),
                Token::Punct("<=") => Some(BinOp::Le),
                Token::Punct(">=") => Some(BinOp::Ge),
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
                let right = self.parse_additive()?;
                left = Expr::Binary(op, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> PResult<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Punct("+") => Some(BinOp::Add),
                Token::Punct("-") => Some(BinOp::Sub),
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
                let right = self.parse_multiplicative()?;
                left = Expr::Binary(op, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> PResult<Expr> {
        let mut left = self.parse_exponent()?;
        loop {
            let op = match self.peek() {
                Token::Punct("*") => Some(BinOp::Mul),
                Token::Punct("/") => Some(BinOp::Div),
                Token::Punct("%") => Some(BinOp::Mod),
                _ => None,
            };
            if let Some(op) = op {
                self.advance();
                let right = self.parse_exponent()?;
                left = Expr::Binary(op, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_exponent(&mut self) -> PResult<Expr> {
        let base = self.parse_unary()?;
        if self.peek_punct("**") {
            self.advance();
            let exp = self.parse_exponent()?; // right-assoc
            Ok(Expr::Binary(BinOp::Mul, Box::new(base), Box::new(exp)))
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        match self.peek() {
            Token::Punct("!") => {
                self.advance();
                Ok(Expr::Unary(UnOp::Not, Box::new(self.parse_unary()?)))
            }
            Token::Punct("-") => {
                self.advance();
                Ok(Expr::Unary(UnOp::Neg, Box::new(self.parse_unary()?)))
            }
            Token::Punct("+") => {
                self.advance();
                let operand = self.parse_unary()?;
                // unary + coerces to number; emulate via double negation.
                Ok(Expr::Unary(
                    UnOp::Neg,
                    Box::new(Expr::Unary(UnOp::Neg, Box::new(operand))),
                ))
            }
            Token::Punct("++") | Token::Punct("--") => {
                let op = match self.advance() {
                    Token::Punct("++") => AssignOp::AddAssign,
                    _ => AssignOp::SubAssign,
                };
                let target = self.parse_unary()?;
                Ok(Expr::Assign(
                    Box::new(target),
                    op,
                    Box::new(Expr::Number(1.0)),
                ))
            }
            Token::Keyword(k) if k == "typeof" => {
                self.advance();
                Ok(Expr::Unary(UnOp::Typeof, Box::new(self.parse_unary()?)))
            }
            Token::Keyword(k) if k == "void" => {
                self.advance();
                let _ = self.parse_unary()?;
                Ok(Expr::Undefined)
            }
            Token::Keyword(k) if k == "new" => self.parse_new(),
            _ => self.parse_postfix(),
        }
    }

    fn parse_new(&mut self) -> PResult<Expr> {
        self.expect_kw("new")?;
        // Parse a member/call chain; if there is a call, mark as New.
        let callee = self.parse_call_member(true)?;
        Ok(callee)
    }

    fn parse_postfix(&mut self) -> PResult<Expr> {
        let e = self.parse_call_member(false)?;
        // postfix ++/--: emit an assignment so loops increment correctly.
        // (Returns the new value rather than the old; acceptable for the subset.)
        if self.peek_punct("++") || self.peek_punct("--") {
            let op = match self.advance() {
                Token::Punct("++") => AssignOp::AddAssign,
                _ => AssignOp::SubAssign,
            };
            return Ok(Expr::Assign(Box::new(e), op, Box::new(Expr::Number(1.0))));
        }
        Ok(e)
    }

    fn parse_call_member(&mut self, new_expr: bool) -> PResult<Expr> {
        let mut e = self.parse_primary()?;
        let mut saw_new = new_expr;
        loop {
            match self.peek() {
                Token::Punct(".") => {
                    self.advance();
                    let prop = match self.advance() {
                        Token::Ident(s) => s,
                        Token::Keyword(s) => s,
                        other => return Err(ParseError(format!("expected property after '.', got {:?}", other))),
                    };
                    e = Expr::Member(Box::new(e), prop);
                }
                Token::Punct("[") => {
                    self.advance();
                    let idx = self.parse_expr()?;
                    self.expect_punct("]")?;
                    e = Expr::Index(Box::new(e), Box::new(idx));
                }
                Token::Punct("(") => {
                    let args = self.parse_args()?;
                    e = if saw_new {
                        saw_new = false;
                        Expr::New(Box::new(e), args)
                    } else {
                        Expr::Call(Box::new(e), args)
                    };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_args(&mut self) -> PResult<Vec<Expr>> {
        self.expect_punct("(")?;
        let mut args = Vec::new();
        if !self.peek_punct(")") {
            loop {
                args.push(self.parse_assign()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        self.expect_punct(")")?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        match self.peek().clone() {
            Token::Num(n) => {
                self.advance();
                Ok(Expr::Number(n))
            }
            Token::Str(s) => {
                self.advance();
                Ok(Expr::Str(s))
            }
            Token::Template(parts) => {
                self.advance();
                let mut out = Vec::with_capacity(parts.len());
                for p in parts {
                    match p {
                        TmplTokenPart::Text(t) => out.push(TmplPart::Text(t)),
                        TmplTokenPart::Expr(src) => {
                            let mut sub = Parser::new(&src)?;
                            let e = sub.parse_expr()?;
                            // trailing semicolons OK
                            out.push(TmplPart::Expr(Box::new(e)));
                        }
                    }
                }
                Ok(Expr::Template(out))
            }
            Token::Keyword(k) => {
                self.advance();
                match k.as_str() {
                    "true" => Ok(Expr::Bool(true)),
                    "false" => Ok(Expr::Bool(false)),
                    "null" => Ok(Expr::Null),
                    "undefined" => Ok(Expr::Undefined),
                    "this" => Ok(Expr::This),
                    "function" => self.parse_func_expr(),
                    "new" => self.parse_new(),
                    _ => Err(ParseError(format!("unexpected keyword `{}`", k))),
                }
            }
            Token::Ident(_) => {
                self.advance();
                // re-read name
                if let Some(Token::Ident(name)) = self.toks.get(self.pos - 1) {
                    Ok(Expr::Ident(name.clone()))
                } else {
                    unreachable!()
                }
            }
            Token::Punct("(") => {
                self.advance();
                let e = self.parse_expr()?;
                self.expect_punct(")")?;
                Ok(e)
            }
            Token::Punct("[") => {
                self.advance();
                let mut items = Vec::new();
                if !self.peek_punct("]") {
                    loop {
                        if self.peek_punct("]") {
                            break;
                        }
                        // allow elision
                        if self.peek_punct(",") {
                            items.push(Expr::Undefined);
                            self.advance();
                            continue;
                        }
                        items.push(self.parse_assign()?);
                        if !self.eat_punct(",") {
                            break;
                        }
                    }
                }
                self.expect_punct("]")?;
                Ok(Expr::Array(items))
            }
            Token::Punct("{") => {
                self.advance();
                let mut props = Vec::new();
                if !self.peek_punct("}") {
                    loop {
                        if self.peek_punct("}") {
                            break;
                        }
                        let key = match self.advance() {
                            Token::Ident(s) => s,
                            Token::Keyword(s) => s,
                            Token::Str(s) => s,
                            Token::Num(n) => n.to_string(),
                            other => {
                                return Err(ParseError(format!(
                                    "bad object key {:?}",
                                    other
                                )))
                            }
                        };
                        let val = if self.eat_punct(":") {
                            self.parse_assign()?
                        } else {
                            Expr::Ident(key.clone())
                        };
                        props.push((key, val));
                        if !self.eat_punct(",") {
                            break;
                        }
                    }
                }
                self.expect_punct("}")?;
                Ok(Expr::Object(props))
            }
            other => Err(ParseError(format!("unexpected token {:?}", other))),
        }
    }

    fn parse_func_expr(&mut self) -> PResult<Expr> {
        self.expect_kw("function")?;
        let name = if matches!(self.peek(), Token::Ident(_)) {
            Some(self.parse_ident()?)
        } else {
            None
        };
        self.expect_punct("(")?;
        let params = self.parse_params()?;
        self.expect_punct("{")?;
        let body = self.parse_block_body()?;
        Ok(Expr::Func(name, params, body))
    }
}

/// Parse a source string into a list of statements.
pub fn parse(src: &str) -> Result<Vec<Stmt>, ParseError> {
    let mut p = Parser::new(src)?;
    p.parse_program()
}

/// Parse a source string as a single expression (used for template `${...}`).
#[allow(dead_code)]
pub fn parse_expression(src: &str) -> Result<Expr, ParseError> {
    let mut p = Parser::new(src)?;
    let e = p.parse_expr()?;
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_var_and_call() {
        let stmts = parse("var x = 'hi'; document.write(x);").unwrap();
        assert_eq!(stmts.len(), 2);
        assert!(matches!(&stmts[0], Stmt::Var { kind: VarKind::Var, .. }));
        assert!(matches!(&stmts[1], Stmt::Expr(Expr::Call(..))));
    }

    #[test]
    fn parse_if_else() {
        let s = parse("if (a > 1) { f(); } else { g(); }").unwrap();
        assert!(matches!(&s[0], Stmt::If { .. }));
    }

    #[test]
    fn parse_for_of() {
        let s = parse("for (var item of items) { document.write(item); }").unwrap();
        assert!(matches!(&s[0], Stmt::ForOf { .. }));
    }

    #[test]
    fn parse_template_member_call() {
        let s = parse("document.write(`<p>${name.toUpperCase()}</p>`);").unwrap();
        match &s[0] {
            Stmt::Expr(Expr::Call(_, args)) => match &args[0] {
                Expr::Template(_) => {}
                other => panic!("expected template arg, got {:?}", other),
            },
            other => panic!("expected call stmt, got {:?}", other),
        }
    }

    #[test]
    fn parse_function_decl() {
        let s = parse("function add(a, b) { return a + b; }").unwrap();
        assert!(matches!(&s[0], Stmt::Func { .. }));
    }

    #[test]
    fn parse_object_literal() {
        let s = parse("var o = { a: 1, b: 'x' };").unwrap();
        match &s[0] {
            Stmt::Var { decls, .. } => match &decls[0].1 {
                Some(Expr::Object(_)) => {}
                other => panic!("expected object, got {:?}", other),
            },
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn parse_operator_precedence() {
        let s = parse("var z = 1 + 2 * 3;").unwrap();
        match &s[0] {
            Stmt::Var { decls, .. } => match &decls[0].1 {
                Some(Expr::Binary(BinOp::Add, _, right)) => {
                    assert!(matches!(**right, Expr::Binary(BinOp::Mul, _, _)));
                }
                other => panic!("expected add, got {:?}", other),
            },
            other => panic!("{:?}", other),
        }
    }
}
