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

use std::rc::Rc;

use rlox::{Expr, FunctionDecl, Stmt, Token, TokenType, Value};

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

// ---- chapter 9: If / While / Logical display ----

#[test]
fn logical_expr_displays_with_operator_lexeme() {
    let or_expr = Expr::Logical {
        left: Box::new(Expr::Literal(Value::Nil)),
        op: Token::new(TokenType::Or, "or", None, 1),
        right: Box::new(Expr::Literal(Value::Bool(true))),
    };
    assert_eq!(or_expr.to_string(), "(or nil true)");
}

#[test]
fn if_stmt_display_with_and_without_else() {
    let then_branch = Box::new(Stmt::Print(n(1.0)));
    let else_branch = Box::new(Stmt::Print(n(2.0)));

    let no_else = Stmt::If {
        condition: Expr::Literal(Value::Bool(true)),
        then_branch: then_branch.clone(),
        else_branch: None,
    };
    assert_eq!(no_else.to_string(), "(if true (print 1.0))");

    let with_else = Stmt::If {
        condition: Expr::Literal(Value::Bool(false)),
        then_branch,
        else_branch: Some(else_branch),
    };
    assert_eq!(with_else.to_string(), "(if false (print 1.0) (print 2.0))");
}

#[test]
fn while_stmt_display() {
    let s = Stmt::While {
        condition: Expr::Literal(Value::Bool(true)),
        body: Box::new(Stmt::Print(n(1.0))),
    };
    assert_eq!(s.to_string(), "(while true (print 1.0))");
}

// ---- chapter 10: Call / Function / Return display ----

#[test]
fn call_expr_display_with_no_arguments() {
    let e = Expr::Call {
        callee: Box::new(Expr::Variable(name("clock"))),
        paren: Token::new(TokenType::RightParen, ")", None, 1),
        arguments: vec![],
    };
    assert_eq!(e.to_string(), "(call clock)");
}

#[test]
fn call_expr_display_with_arguments() {
    let e = Expr::Call {
        callee: Box::new(Expr::Variable(name("add"))),
        paren: Token::new(TokenType::RightParen, ")", None, 1),
        arguments: vec![n(1.0), n(2.0)],
    };
    assert_eq!(e.to_string(), "(call add 1.0 2.0)");
}

#[test]
fn function_stmt_display_with_no_params() {
    let s = Stmt::Function(Rc::new(FunctionDecl {
        name: name("greet"),
        params: vec![],
        body: vec![Stmt::Print(Expr::Literal(Value::String("hi".into())))],
    }));
    assert_eq!(s.to_string(), "(fun greet () (print hi))");
}

#[test]
fn function_stmt_display_with_params_and_body() {
    let s = Stmt::Function(Rc::new(FunctionDecl {
        name: name("add"),
        params: vec![name("a"), name("b")],
        body: vec![Stmt::Return {
            keyword: Token::new(TokenType::Return, "return", None, 1),
            value: Some(Expr::Variable(name("a"))),
        }],
    }));
    assert_eq!(s.to_string(), "(fun add (a b) (return a))");
}

#[test]
fn return_stmt_display_with_and_without_value() {
    let bare = Stmt::Return {
        keyword: Token::new(TokenType::Return, "return", None, 1),
        value: None,
    };
    let valued = Stmt::Return {
        keyword: Token::new(TokenType::Return, "return", None, 1),
        value: Some(n(1.0)),
    };
    assert_eq!(bare.to_string(), "(return)");
    assert_eq!(valued.to_string(), "(return 1.0)");
}

// ---- chapter 12: Class / Get / Set / This display ----

#[test]
fn get_expr_display() {
    let e = Expr::Get {
        object: Box::new(Expr::Variable(name("a"))),
        name: name("b"),
    };
    assert_eq!(e.to_string(), "(. a b)");
}

#[test]
fn set_expr_display() {
    let e = Expr::Set {
        object: Box::new(Expr::Variable(name("a"))),
        name: name("b"),
        value: Box::new(n(1.0)),
    };
    assert_eq!(e.to_string(), "(.= a b 1.0)");
}

#[test]
fn this_expr_display() {
    let e = Expr::This(Token::new(TokenType::This, "this", None, 1));
    assert_eq!(e.to_string(), "this");
}

#[test]
fn empty_class_stmt_display() {
    let s = Stmt::Class {
        name: name("Foo"),
        superclass: None,
        methods: vec![],
    };
    assert_eq!(s.to_string(), "(class Foo)");
}

#[test]
fn class_stmt_display_with_methods() {
    let s = Stmt::Class {
        name: name("Greeter"),
        superclass: None,
        methods: vec![Rc::new(FunctionDecl {
            name: name("greet"),
            params: vec![],
            body: vec![Stmt::Print(Expr::Literal(Value::String("hi".into())))],
        })],
    };
    assert_eq!(
        s.to_string(),
        "(class Greeter (method greet () (print hi)))"
    );
}

// ---- chapter 13: superclass / super display ----

#[test]
fn super_expr_display() {
    let e = Expr::Super {
        keyword: Token::new(TokenType::Super, "super", None, 1),
        method: name("greet"),
    };
    assert_eq!(e.to_string(), "(super greet)");
}

#[test]
fn class_stmt_display_with_superclass() {
    let s = Stmt::Class {
        name: name("Sub"),
        superclass: Some(Expr::Variable(name("Sup"))),
        methods: vec![],
    };
    assert_eq!(s.to_string(), "(class Sub < Sup)");
}
