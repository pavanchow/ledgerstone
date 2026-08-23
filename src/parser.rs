//! A recursive-descent parser turning tokens into a small statement AST.
//! The only place recursion can grow with input is parenthesized WHERE
//! expressions, and that is capped by `MAX_EXPR_DEPTH` so a hostile string
//! of open parens returns a syntax error instead of overflowing the stack.

use crate::error::{LsError, LsResult};
use crate::lexer::Token;
use crate::types::{ColumnType, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum CompareOp {
    Eq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Compare {
        column: String,
        op: CompareOp,
        value: Value,
    },
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub name: String,
    pub ty: ColumnType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderBy {
    pub column: String,
    pub dir: OrderDir,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SelectColumns {
    All,
    Named(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    CreateTable {
        table: String,
        columns: Vec<ColumnDef>,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        values: Vec<Value>,
    },
    Select {
        table: String,
        columns: SelectColumns,
        where_clause: Option<Expr>,
        order_by: Option<OrderBy>,
        limit: Option<u64>,
    },
    Delete {
        table: String,
        where_clause: Option<Expr>,
    },
    Update {
        table: String,
        assignments: Vec<(String, Value)>,
        where_clause: Option<Expr>,
    },
}

const MAX_EXPR_DEPTH: usize = 64;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}

pub fn parse(tokens: Vec<Token>) -> LsResult<Statement> {
    let mut p = Parser {
        tokens,
        pos: 0,
        depth: 0,
    };
    let stmt = p.parse_statement()?;
    p.expect_end()?;
    Ok(stmt)
}

impl Parser {
    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, tok: &Token) -> LsResult<()> {
        if self.peek() == tok {
            self.advance();
            Ok(())
        } else {
            Err(LsError::Syntax(format!(
                "expected {tok:?}, found {:?}",
                self.peek()
            )))
        }
    }

    fn expect_end(&mut self) -> LsResult<()> {
        // Allow a single trailing semicolon before end of input.
        if self.peek() == &Token::Semicolon {
            self.advance();
        }
        if self.peek() == &Token::Eof {
            Ok(())
        } else {
            Err(LsError::Syntax(format!(
                "unexpected trailing input near {:?}",
                self.peek()
            )))
        }
    }

    fn ident(&mut self) -> LsResult<String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            other => Err(LsError::Syntax(format!("expected identifier, found {other:?}"))),
        }
    }

    fn parse_statement(&mut self) -> LsResult<Statement> {
        match self.peek().clone() {
            Token::Create => self.parse_create_table(),
            Token::Insert => self.parse_insert(),
            Token::Select => self.parse_select(),
            Token::Delete => self.parse_delete(),
            Token::Update => self.parse_update(),
            other => Err(LsError::Syntax(format!(
                "expected a statement (CREATE, INSERT, SELECT, DELETE, UPDATE), found {other:?}"
            ))),
        }
    }

    fn parse_create_table(&mut self) -> LsResult<Statement> {
        self.expect(&Token::Create)?;
        self.expect(&Token::Table)?;
        let table = self.ident()?;
        self.expect(&Token::LParen)?;
        let mut columns = Vec::new();
        loop {
            let name = self.ident()?;
            let ty = match self.advance() {
                Token::Integer => ColumnType::Integer,
                Token::Text => ColumnType::Text,
                Token::Real => ColumnType::Real,
                other => {
                    return Err(LsError::Syntax(format!(
                        "expected a column type (INTEGER, TEXT, REAL), found {other:?}"
                    )))
                }
            };
            columns.push(ColumnDef { name, ty });
            match self.peek() {
                Token::Comma => {
                    self.advance();
                }
                Token::RParen => break,
                other => {
                    return Err(LsError::Syntax(format!(
                        "expected ',' or ')' in column list, found {other:?}"
                    )))
                }
            }
        }
        self.expect(&Token::RParen)?;
        if columns.is_empty() {
            return Err(LsError::Syntax("CREATE TABLE needs at least one column".into()));
        }
        Ok(Statement::CreateTable { table, columns })
    }

    fn parse_insert(&mut self) -> LsResult<Statement> {
        self.expect(&Token::Insert)?;
        self.expect(&Token::Into)?;
        let table = self.ident()?;

        let columns = if self.peek() == &Token::LParen {
            self.advance();
            let mut cols = Vec::new();
            loop {
                cols.push(self.ident()?);
                match self.peek() {
                    Token::Comma => {
                        self.advance();
                    }
                    Token::RParen => break,
                    other => {
                        return Err(LsError::Syntax(format!(
                            "expected ',' or ')' in column list, found {other:?}"
                        )))
                    }
                }
            }
            self.expect(&Token::RParen)?;
            Some(cols)
        } else {
            None
        };

        self.expect(&Token::Values)?;
        self.expect(&Token::LParen)?;
        let mut values = Vec::new();
        loop {
            values.push(self.parse_literal()?);
            match self.peek() {
                Token::Comma => {
                    self.advance();
                }
                Token::RParen => break,
                other => {
                    return Err(LsError::Syntax(format!(
                        "expected ',' or ')' in VALUES list, found {other:?}"
                    )))
                }
            }
        }
        self.expect(&Token::RParen)?;
        Ok(Statement::Insert {
            table,
            columns,
            values,
        })
    }

    fn parse_select(&mut self) -> LsResult<Statement> {
        self.expect(&Token::Select)?;
        let columns = if self.peek() == &Token::Star {
            self.advance();
            SelectColumns::All
        } else {
            let mut cols = vec![self.ident()?];
            while self.peek() == &Token::Comma {
                self.advance();
                cols.push(self.ident()?);
            }
            SelectColumns::Named(cols)
        };
        self.expect(&Token::From)?;
        let table = self.ident()?;

        let where_clause = if self.peek() == &Token::Where {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        let order_by = if self.peek() == &Token::Order {
            self.advance();
            self.expect(&Token::By)?;
            let column = self.ident()?;
            let dir = match self.peek() {
                Token::Asc => {
                    self.advance();
                    OrderDir::Asc
                }
                Token::Desc => {
                    self.advance();
                    OrderDir::Desc
                }
                _ => OrderDir::Asc,
            };
            Some(OrderBy { column, dir })
        } else {
            None
        };

        let limit = if self.peek() == &Token::Limit {
            self.advance();
            match self.advance() {
                Token::IntLit(n) if n >= 0 => Some(n as u64),
                other => {
                    return Err(LsError::Syntax(format!(
                        "expected a non-negative integer after LIMIT, found {other:?}"
                    )))
                }
            }
        } else {
            None
        };

        Ok(Statement::Select {
            table,
            columns,
            where_clause,
            order_by,
            limit,
        })
    }

    fn parse_delete(&mut self) -> LsResult<Statement> {
        self.expect(&Token::Delete)?;
        self.expect(&Token::From)?;
        let table = self.ident()?;
        let where_clause = if self.peek() == &Token::Where {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Delete { table, where_clause })
    }

    fn parse_update(&mut self) -> LsResult<Statement> {
        self.expect(&Token::Update)?;
        let table = self.ident()?;
        self.expect(&Token::Set)?;
        let mut assignments = Vec::new();
        loop {
            let col = self.ident()?;
            self.expect(&Token::Eq)?;
            let val = self.parse_literal()?;
            assignments.push((col, val));
            if self.peek() == &Token::Comma {
                self.advance();
            } else {
                break;
            }
        }
        let where_clause = if self.peek() == &Token::Where {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Statement::Update {
            table,
            assignments,
            where_clause,
        })
    }

    fn parse_literal(&mut self) -> LsResult<Value> {
        match self.advance() {
            Token::IntLit(n) => Ok(Value::Integer(n)),
            Token::RealLit(n) => Ok(Value::Real(n)),
            Token::StrLit(s) => Ok(Value::Text(s)),
            other => Err(LsError::Syntax(format!("expected a literal value, found {other:?}"))),
        }
    }

    // or_expr := and_expr (OR and_expr)*
    fn parse_expr(&mut self) -> LsResult<Expr> {
        self.enter()?;
        let mut left = self.parse_and()?;
        while self.peek() == &Token::Or {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        self.leave();
        Ok(left)
    }

    // and_expr := primary (AND primary)*
    fn parse_and(&mut self) -> LsResult<Expr> {
        self.enter()?;
        let mut left = self.parse_primary()?;
        while self.peek() == &Token::And {
            self.advance();
            let right = self.parse_primary()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        self.leave();
        Ok(left)
    }

    fn parse_primary(&mut self) -> LsResult<Expr> {
        if self.peek() == &Token::LParen {
            self.advance();
            let inner = self.parse_expr()?;
            self.expect(&Token::RParen)?;
            return Ok(inner);
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> LsResult<Expr> {
        let column = self.ident()?;
        let op = match self.advance() {
            Token::Eq => CompareOp::Eq,
            Token::NotEq => CompareOp::NotEq,
            Token::Lt => CompareOp::Lt,
            Token::Gt => CompareOp::Gt,
            Token::Le => CompareOp::Le,
            Token::Ge => CompareOp::Ge,
            other => {
                return Err(LsError::Syntax(format!(
                    "expected a comparison operator, found {other:?}"
                )))
            }
        };
        let value = self.parse_literal()?;
        Ok(Expr::Compare { column, op, value })
    }

    fn enter(&mut self) -> LsResult<()> {
        self.depth += 1;
        if self.depth > MAX_EXPR_DEPTH {
            return Err(LsError::Syntax("WHERE expression nested too deeply".into()));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
}
