//! Phase 2 — public-API tests for `scanner` module.
//!
//! Reference cases are transcribed from the upstream Crafting Interpreters
//! test suite (e.g. `test/scanning/numbers.lox`). The scanner is exposed via
//! `rlox::scan(source)` which returns `(tokens, errors)` so callers can
//! continue past lexical errors (matching jlox's behaviour).

use rlox::{Literal, LoxError, TokenType, scan};

fn types(tokens: &[rlox::Token]) -> Vec<TokenType> {
    tokens.iter().map(|t| t.ttype).collect()
}

fn dump(tokens: &[rlox::Token]) -> String {
    tokens
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn empty_source_yields_only_eof() {
    let (tokens, errors) = scan("");
    assert!(errors.is_empty());
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].ttype, TokenType::Eof);
    assert_eq!(tokens[0].line, 1);
    assert_eq!(tokens[0].lexeme, "");
    assert_eq!(tokens[0].literal, None);
}

#[test]
fn scans_single_char_punctuators() {
    let (tokens, errors) = scan("(){},.-+;*");
    assert!(errors.is_empty());
    assert_eq!(
        types(&tokens),
        vec![
            TokenType::LeftParen,
            TokenType::RightParen,
            TokenType::LeftBrace,
            TokenType::RightBrace,
            TokenType::Comma,
            TokenType::Dot,
            TokenType::Minus,
            TokenType::Plus,
            TokenType::Semicolon,
            TokenType::Star,
            TokenType::Eof,
        ]
    );
}

#[test]
fn scans_one_and_two_char_operators() {
    let (tokens, errors) = scan("! != = == < <= > >=");
    assert!(errors.is_empty());
    assert_eq!(
        types(&tokens),
        vec![
            TokenType::Bang,
            TokenType::BangEqual,
            TokenType::Equal,
            TokenType::EqualEqual,
            TokenType::Less,
            TokenType::LessEqual,
            TokenType::Greater,
            TokenType::GreaterEqual,
            TokenType::Eof,
        ]
    );
}

#[test]
fn slash_is_a_token_and_double_slash_starts_line_comment() {
    let (tokens, errors) = scan("/ // a comment\n+");
    assert!(errors.is_empty());
    assert_eq!(
        types(&tokens),
        vec![TokenType::Slash, TokenType::Plus, TokenType::Eof],
    );
    assert_eq!(tokens[1].line, 2, "+ is on line 2 after the comment");
}

#[test]
fn skips_whitespace_and_tracks_lines() {
    let (tokens, errors) = scan("\n\n  \tand\n");
    assert!(errors.is_empty());
    assert_eq!(types(&tokens), vec![TokenType::And, TokenType::Eof]);
    assert_eq!(tokens[0].line, 3);
    assert_eq!(tokens[1].line, 4);
}

#[test]
fn scans_string_literal() {
    let (tokens, errors) = scan(r#""hi""#);
    assert!(errors.is_empty());
    assert_eq!(types(&tokens), vec![TokenType::String, TokenType::Eof]);
    assert_eq!(tokens[0].lexeme, "\"hi\"");
    assert_eq!(tokens[0].literal, Some(Literal::String("hi".into())));
}

#[test]
fn multiline_string_literal_advances_line_counter() {
    let (tokens, errors) = scan("\"a\nb\"\n+");
    assert!(errors.is_empty());
    assert_eq!(
        types(&tokens),
        vec![TokenType::String, TokenType::Plus, TokenType::Eof],
    );
    // The STRING token reports the line of its *closing* quote (matches
    // jlox: `line` advances as the scanner consumes embedded newlines).
    assert_eq!(tokens[0].line, 2);
    assert_eq!(tokens[0].literal, Some(Literal::String("a\nb".into())));
    // `+` is on line 3 (after the trailing newline that follows the string).
    assert_eq!(tokens[1].line, 3);
}

#[test]
fn unterminated_string_records_scan_error() {
    let (tokens, errors) = scan("\"abc");
    assert_eq!(errors.len(), 1);
    let LoxError::Scan { line, message } = &errors[0] else {
        panic!("expected Scan error, got {:?}", errors[0]);
    };
    assert_eq!(*line, 1);
    assert_eq!(message, "Unterminated string.");
    // Even on error the scanner still emits an EOF terminator.
    assert_eq!(types(&tokens), vec![TokenType::Eof]);
}

#[test]
fn scans_integer_number_literal() {
    let (tokens, errors) = scan("123");
    assert!(errors.is_empty());
    assert_eq!(types(&tokens), vec![TokenType::Number, TokenType::Eof]);
    assert_eq!(tokens[0].lexeme, "123");
    assert_eq!(tokens[0].literal, Some(Literal::Number(123.0)));
}

#[test]
fn scans_decimal_number_literal() {
    let (tokens, errors) = scan("123.456");
    assert!(errors.is_empty());
    assert_eq!(types(&tokens), vec![TokenType::Number, TokenType::Eof]);
    assert_eq!(tokens[0].lexeme, "123.456");
    assert_eq!(tokens[0].literal, Some(Literal::Number(123.456)));
}

#[test]
fn trailing_dot_does_not_join_into_number() {
    // From upstream `test/scanning/numbers.lox`: `123.` -> NUMBER 123, DOT.
    let (tokens, errors) = scan("123.");
    assert!(errors.is_empty());
    assert_eq!(
        types(&tokens),
        vec![TokenType::Number, TokenType::Dot, TokenType::Eof],
    );
    assert_eq!(tokens[0].literal, Some(Literal::Number(123.0)));
}

#[test]
fn leading_dot_does_not_join_into_number() {
    // From upstream `test/scanning/numbers.lox`: `.456` -> DOT, NUMBER 456.
    let (tokens, errors) = scan(".456");
    assert!(errors.is_empty());
    assert_eq!(
        types(&tokens),
        vec![TokenType::Dot, TokenType::Number, TokenType::Eof],
    );
    assert_eq!(tokens[1].literal, Some(Literal::Number(456.0)));
}

#[test]
fn scans_identifiers_and_distinguishes_keywords() {
    let (tokens, errors) = scan(
        "orchid orchidaceae and class else false fun for if nil or print return super this true var while",
    );
    assert!(errors.is_empty());
    assert_eq!(
        types(&tokens),
        vec![
            TokenType::Identifier,
            TokenType::Identifier,
            TokenType::And,
            TokenType::Class,
            TokenType::Else,
            TokenType::False,
            TokenType::Fun,
            TokenType::For,
            TokenType::If,
            TokenType::Nil,
            TokenType::Or,
            TokenType::Print,
            TokenType::Return,
            TokenType::Super,
            TokenType::This,
            TokenType::True,
            TokenType::Var,
            TokenType::While,
            TokenType::Eof,
        ]
    );
    assert_eq!(tokens[0].lexeme, "orchid");
    assert_eq!(tokens[1].lexeme, "orchidaceae");
}

#[test]
fn unexpected_character_records_scan_error_and_keeps_going() {
    let (tokens, errors) = scan("@123");
    assert_eq!(errors.len(), 1);
    let LoxError::Scan { line, message } = &errors[0] else {
        panic!("expected Scan error");
    };
    assert_eq!(*line, 1);
    assert_eq!(message, "Unexpected character.");
    // Scanner continues past the bad char and recognises the number.
    assert_eq!(types(&tokens), vec![TokenType::Number, TokenType::Eof]);
}

#[test]
fn dump_format_matches_upstream_numbers_lox() {
    // From <https://github.com/munificent/craftinginterpreters/blob/master/test/scanning/numbers.lox>:
    //
    //     123
    //     123.456
    //     .456
    //     123.
    //
    // Expected dump:
    //     NUMBER 123 123.0
    //     NUMBER 123.456 123.456
    //     DOT . null
    //     NUMBER 456 456.0
    //     NUMBER 123 123.0
    //     DOT . null
    //     EOF  null
    let (tokens, errors) = scan("123\n123.456\n.456\n123.\n");
    assert!(errors.is_empty());
    let expected = "\
NUMBER 123 123.0
NUMBER 123.456 123.456
DOT . null
NUMBER 456 456.0
NUMBER 123 123.0
DOT . null
EOF  null";
    assert_eq!(dump(&tokens), expected);
}
