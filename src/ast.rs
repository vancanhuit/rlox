//! Abstract syntax tree for Lox expressions and statements
//! (chapters 5–6, extended by chapter 8 with statements + variables,
//! chapter 9 with control flow + short-circuit logical operators, and
//! chapter 10 with function declarations, calls, and `return`).
//!
//! We use idiomatic Rust enums + `match` rather than the book's Visitor
//! pattern. The `Display` impls produce a parenthesised, prefix-Lisp form
//! consistent with upstream `test/expressions/parse.lox` for expressions
//! and a natural extension for statements.

use std::fmt;
use std::rc::Rc;

use crate::token::Token;
use crate::value::Value;

/// The static parts of a function declaration. Shared via `Rc` between
/// the original [`Stmt::Function`] node and any [`crate::callable::LoxFunction`]
/// values created from it. Sharing rather than cloning is important for
/// chapter 11 — the resolver records `Expr` addresses inside `body`,
/// and those addresses must stay valid when the function is later called.
#[derive(Debug, PartialEq)]
pub struct FunctionDecl {
    pub name: Token,
    pub params: Vec<Token>,
    pub body: Vec<Stmt>,
}

/// An expression node.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Assign {
        name: Token,
        value: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: Token,
        right: Box<Expr>,
    },
    Grouping(Box<Expr>),
    Literal(Value),
    /// Short-circuit `and` / `or`. Distinct from [`Expr::Binary`] because
    /// the right-hand side is evaluated conditionally and the result is
    /// the operand value (not a coerced boolean).
    Logical {
        left: Box<Expr>,
        op: Token,
        right: Box<Expr>,
    },
    Unary {
        op: Token,
        right: Box<Expr>,
    },
    /// Read of a variable by name (the token carries the source location).
    Variable(Token),
    /// Function call (chapter 10). `paren` is the closing `)` for runtime
    /// error reporting.
    Call {
        callee: Box<Expr>,
        paren: Token,
        arguments: Vec<Expr>,
    },
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Assign { name, value } => write!(f, "(= {} {value})", name.lexeme),
            Self::Binary { left, op, right } | Self::Logical { left, op, right } => {
                parenthesize(f, &op.lexeme, [left.as_ref(), right.as_ref()])
            }
            Self::Grouping(inner) => parenthesize(f, "group", [inner.as_ref()]),
            Self::Literal(value) => write!(f, "{value}"),
            Self::Unary { op, right } => parenthesize(f, &op.lexeme, [right.as_ref()]),
            Self::Variable(name) => f.write_str(&name.lexeme),
            Self::Call {
                callee, arguments, ..
            } => {
                f.write_str("(call ")?;
                write!(f, "{callee}")?;
                for a in arguments {
                    write!(f, " {a}")?;
                }
                f.write_str(")")
            }
        }
    }
}

/// A statement node (chapters 8–9).
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `expr;` — evaluate for side effects and discard the value.
    Expression(Expr),
    /// `print expr;` — evaluate and write the stringified value to the
    /// interpreter's output sink.
    Print(Expr),
    /// `var name [= initializer];`
    Var {
        name: Token,
        initializer: Option<Expr>,
    },
    /// `{ stmts... }` — introduces a new lexical scope.
    Block(Vec<Stmt>),
    /// `if (condition) then_branch [else else_branch]` (chapter 9).
    If {
        condition: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    /// `while (condition) body` (chapter 9). `for` loops are desugared at
    /// parse time into a `Block` containing a `While`, so we don't need a
    /// separate `For` variant.
    While { condition: Expr, body: Box<Stmt> },
    /// `fun name(params) { body }` (chapter 10). Wrapped in `Rc` so the
    /// runtime [`LoxFunction`](crate::callable::LoxFunction) value shares
    /// the same backing AST — keeping `Expr` addresses inside `body`
    /// stable across function calls (the resolver in chapter 11 keys its
    /// locals map on those addresses).
    Function(Rc<FunctionDecl>),
    /// `return [value];` (chapter 10). `keyword` carries the source line
    /// for runtime errors; `value` is `None` when the bare keyword form
    /// is used (returns `nil`).
    Return { keyword: Token, value: Option<Expr> },
}

impl fmt::Display for Stmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Expression(e) => write!(f, "(; {e})"),
            Self::Print(e) => write!(f, "(print {e})"),
            Self::Var { name, initializer } => match initializer {
                Some(e) => write!(f, "(var {} {e})", name.lexeme),
                None => write!(f, "(var {})", name.lexeme),
            },
            Self::Block(stmts) => {
                f.write_str("(block")?;
                for s in stmts {
                    write!(f, " {s}")?;
                }
                f.write_str(")")
            }
            Self::If {
                condition,
                then_branch,
                else_branch,
            } => match else_branch {
                Some(e) => write!(f, "(if {condition} {then_branch} {e})"),
                None => write!(f, "(if {condition} {then_branch})"),
            },
            Self::While { condition, body } => write!(f, "(while {condition} {body})"),
            Self::Function(decl) => {
                f.write_str("(fun ")?;
                f.write_str(&decl.name.lexeme)?;
                f.write_str(" (")?;
                for (i, p) in decl.params.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" ")?;
                    }
                    f.write_str(&p.lexeme)?;
                }
                f.write_str(")")?;
                for s in &decl.body {
                    write!(f, " {s}")?;
                }
                f.write_str(")")
            }
            Self::Return { value, .. } => match value {
                Some(v) => write!(f, "(return {v})"),
                None => f.write_str("(return)"),
            },
        }
    }
}

fn parenthesize<'a, I>(f: &mut fmt::Formatter<'_>, name: &str, exprs: I) -> fmt::Result
where
    I: IntoIterator<Item = &'a Expr>,
{
    f.write_str("(")?;
    f.write_str(name)?;
    for expr in exprs {
        write!(f, " {expr}")?;
    }
    f.write_str(")")
}
