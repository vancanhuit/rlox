//! Phase 3 — public-API tests for the `Expr` AST and its `Display` impl.
//!
//! Reference: chapter 6 test `test/expressions/parse.lox` from the upstream
//! Crafting Interpreters corpus:
//!
//! ```text
//! (5 - (3 - 1)) + -1
//! // expect: (+ (group (- 5.0 (group (- 3.0 1.0)))) (- 1.0))
//! ```
//!
//! The AST `Display` impl produces the same parenthesised, prefix-Lisp form.

use rlox::{Expr, Stmt, Token, TokenType, Value};

fn op(ttype: TokenType, lexeme: &'static str) -> Token {
    Token::new(ttype, lexeme, None, 1)
}

fn n(x: f64) -> Expr {
    Expr::Literal(Value::Number(x))
}

#[test]
fn value_display_renders_atoms() {
    assert_eq!(Value::Nil.to_string(), "nil");
    assert_eq!(Value::Bool(true).to_string(), "true");
    assert_eq!(Value::Bool(false).to_string(), "false");
    assert_eq!(Value::Number(5.0).to_string(), "5.0");
    assert_eq!(Value::Number(5.5).to_string(), "5.5");
    assert_eq!(Value::String("hi".into()).to_string(), "hi");
}

#[test]
fn literal_expr_uses_value_display() {
    assert_eq!(Expr::Literal(Value::Number(1.0)).to_string(), "1.0");
    assert_eq!(Expr::Literal(Value::Nil).to_string(), "nil");
    assert_eq!(Expr::Literal(Value::Bool(true)).to_string(), "true");
}

#[test]
fn unary_expr_uses_operator_lexeme() {
    let e = Expr::Unary {
        op: op(TokenType::Minus, "-"),
        right: Box::new(n(1.0)),
    };
    assert_eq!(e.to_string(), "(- 1.0)");

    let bang = Expr::Unary {
        op: op(TokenType::Bang, "!"),
        right: Box::new(Expr::Literal(Value::Bool(true))),
    };
    assert_eq!(bang.to_string(), "(! true)");
}

#[test]
fn binary_expr_uses_operator_lexeme() {
    let e = Expr::Binary {
        left: Box::new(n(1.0)),
        op: op(TokenType::Plus, "+"),
        right: Box::new(n(2.0)),
    };
    assert_eq!(e.to_string(), "(+ 1.0 2.0)");
}

#[test]
fn grouping_expr_renders_with_group_keyword() {
    let inner = Expr::Binary {
        left: Box::new(n(1.0)),
        op: op(TokenType::Plus, "+"),
        right: Box::new(n(2.0)),
    };
    let g = Expr::Grouping(Box::new(inner));
    assert_eq!(g.to_string(), "(group (+ 1.0 2.0))");
}

#[test]
fn parse_dot_lox_reference_case() {
    // Manual AST for `(5 - (3 - 1)) + -1`.
    let three_minus_one = Expr::Binary {
        left: Box::new(n(3.0)),
        op: op(TokenType::Minus, "-"),
        right: Box::new(n(1.0)),
    };
    let inner_group = Expr::Grouping(Box::new(three_minus_one));
    let five_minus_inner = Expr::Binary {
        left: Box::new(n(5.0)),
        op: op(TokenType::Minus, "-"),
        right: Box::new(inner_group),
    };
    let outer_group = Expr::Grouping(Box::new(five_minus_inner));
    let neg_one = Expr::Unary {
        op: op(TokenType::Minus, "-"),
        right: Box::new(n(1.0)),
    };
    let whole = Expr::Binary {
        left: Box::new(outer_group),
        op: op(TokenType::Plus, "+"),
        right: Box::new(neg_one),
    };

    assert_eq!(
        whole.to_string(),
        "(+ (group (- 5.0 (group (- 3.0 1.0)))) (- 1.0))"
    );
}

// ---- chapter 8: new Expr variants + Stmt Display ----

fn name(s: &'static str) -> Token {
    Token::new(TokenType::Identifier, s, None, 1)
}

#[test]
fn variable_expr_displays_as_bare_identifier() {
    assert_eq!(Expr::Variable(name("a")).to_string(), "a");
}

#[test]
fn assign_expr_displays_in_prefix_form() {
    let e = Expr::Assign {
        name: name("a"),
        value: Box::new(n(1.0)),
    };
    assert_eq!(e.to_string(), "(= a 1.0)");
}

#[test]
fn print_stmt_display() {
    assert_eq!(Stmt::Print(n(1.0)).to_string(), "(print 1.0)");
}

#[test]
fn expression_stmt_display_uses_semicolon_marker() {
    assert_eq!(Stmt::Expression(n(1.0)).to_string(), "(; 1.0)");
}

#[test]
fn var_stmt_display_with_and_without_initializer() {
    let with = Stmt::Var {
        name: name("a"),
        initializer: Some(n(1.0)),
    };
    let without = Stmt::Var {
        name: name("b"),
        initializer: None,
    };
    assert_eq!(with.to_string(), "(var a 1.0)");
    assert_eq!(without.to_string(), "(var b)");
}

#[test]
fn block_stmt_display_lists_inner_statements() {
    let block = Stmt::Block(vec![
        Stmt::Var {
            name: name("a"),
            initializer: Some(n(1.0)),
        },
        Stmt::Print(Expr::Variable(name("a"))),
    ]);
    assert_eq!(block.to_string(), "(block (var a 1.0) (print a))");
}
