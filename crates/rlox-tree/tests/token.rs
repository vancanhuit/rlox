//! Phase 1 — public-API tests for `token` module.
//!
//! Reference: chapter 4 token output format from the upstream `crafting
//! interpreters` test suite, e.g. `test/scanning/numbers.lox`:
//!     NUMBER 123 123.0
//!     DOT . null
//!     EOF  null

use rlox_tree::{Literal, Token, TokenType};

#[test]
fn token_type_display_matches_upstream_dump_format() {
    assert_eq!(TokenType::LeftParen.to_string(), "LEFT_PAREN");
    assert_eq!(TokenType::RightParen.to_string(), "RIGHT_PAREN");
    assert_eq!(TokenType::LeftBrace.to_string(), "LEFT_BRACE");
    assert_eq!(TokenType::RightBrace.to_string(), "RIGHT_BRACE");
    assert_eq!(TokenType::Comma.to_string(), "COMMA");
    assert_eq!(TokenType::Dot.to_string(), "DOT");
    assert_eq!(TokenType::Minus.to_string(), "MINUS");
    assert_eq!(TokenType::Plus.to_string(), "PLUS");
    assert_eq!(TokenType::Semicolon.to_string(), "SEMICOLON");
    assert_eq!(TokenType::Slash.to_string(), "SLASH");
    assert_eq!(TokenType::Star.to_string(), "STAR");

    assert_eq!(TokenType::Bang.to_string(), "BANG");
    assert_eq!(TokenType::BangEqual.to_string(), "BANG_EQUAL");
    assert_eq!(TokenType::Equal.to_string(), "EQUAL");
    assert_eq!(TokenType::EqualEqual.to_string(), "EQUAL_EQUAL");
    assert_eq!(TokenType::Greater.to_string(), "GREATER");
    assert_eq!(TokenType::GreaterEqual.to_string(), "GREATER_EQUAL");
    assert_eq!(TokenType::Less.to_string(), "LESS");
    assert_eq!(TokenType::LessEqual.to_string(), "LESS_EQUAL");

    assert_eq!(TokenType::Identifier.to_string(), "IDENTIFIER");
    assert_eq!(TokenType::String.to_string(), "STRING");
    assert_eq!(TokenType::Number.to_string(), "NUMBER");

    assert_eq!(TokenType::And.to_string(), "AND");
    assert_eq!(TokenType::Class.to_string(), "CLASS");
    assert_eq!(TokenType::Else.to_string(), "ELSE");
    assert_eq!(TokenType::False.to_string(), "FALSE");
    assert_eq!(TokenType::Fun.to_string(), "FUN");
    assert_eq!(TokenType::For.to_string(), "FOR");
    assert_eq!(TokenType::If.to_string(), "IF");
    assert_eq!(TokenType::Nil.to_string(), "NIL");
    assert_eq!(TokenType::Or.to_string(), "OR");
    assert_eq!(TokenType::Print.to_string(), "PRINT");
    assert_eq!(TokenType::Return.to_string(), "RETURN");
    assert_eq!(TokenType::Super.to_string(), "SUPER");
    assert_eq!(TokenType::This.to_string(), "THIS");
    assert_eq!(TokenType::True.to_string(), "TRUE");
    assert_eq!(TokenType::Var.to_string(), "VAR");
    assert_eq!(TokenType::While.to_string(), "WHILE");

    assert_eq!(TokenType::Eof.to_string(), "EOF");
}

#[test]
fn literal_display_matches_book_format() {
    // Whole numbers render with trailing ".0" (matches `NUMBER 123 123.0`
    // in upstream `test/scanning/numbers.lox`).
    assert_eq!(Literal::Number(123.0).to_string(), "123.0");
    assert_eq!(Literal::Number(0.0).to_string(), "0.0");
    // Fractions render naturally.
    assert_eq!(Literal::Number(123.456).to_string(), "123.456");
    assert_eq!(Literal::Number(0.5).to_string(), "0.5");
    // Strings render without surrounding quotes.
    assert_eq!(Literal::String("hi".into()).to_string(), "hi");
    assert_eq!(Literal::String(String::new()).to_string(), "");
}

#[test]
fn token_constructor_round_trips_fields() {
    let tok = Token::new(TokenType::Number, "123", Some(Literal::Number(123.0)), 7);
    assert_eq!(tok.ttype, TokenType::Number);
    assert_eq!(tok.lexeme, "123");
    assert_eq!(tok.literal, Some(Literal::Number(123.0)));
    assert_eq!(tok.line, 7);
}

#[test]
fn token_dump_format_matches_upstream() {
    // From `test/scanning/numbers.lox`:
    //     NUMBER 123 123.0
    let number_tok = Token::new(TokenType::Number, "123", Some(Literal::Number(123.0)), 1);
    assert_eq!(number_tok.to_string(), "NUMBER 123 123.0");

    //     DOT . null
    let dot_tok = Token::new(TokenType::Dot, ".", None, 1);
    assert_eq!(dot_tok.to_string(), "DOT . null");

    //     EOF  null
    // (note the double space — empty lexeme)
    let eof_tok = Token::new(TokenType::Eof, "", None, 1);
    assert_eq!(eof_tok.to_string(), "EOF  null");

    //     STRING "hi" hi
    let string_tok = Token::new(
        TokenType::String,
        "\"hi\"",
        Some(Literal::String("hi".into())),
        1,
    );
    assert_eq!(string_tok.to_string(), "STRING \"hi\" hi");
}
