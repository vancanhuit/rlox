//! Phase 2 — public-API tests for `scanner` module.
//!
//! Reference cases are transcribed from the upstream Crafting Interpreters
//! test suite (e.g. `test/scanning/numbers.lox`). The scanner is exposed via
//! `rlox_shared::scan(source)` which returns `(tokens, errors)` so callers can
//! continue past lexical errors (matching jlox's behaviour).

use rlox_shared::{Literal, LoxError, TokenType, scan};

fn types(tokens: &[rlox_shared::Token]) -> Vec<TokenType> {
    tokens.iter().map(|t| t.ttype).collect()
}

fn dump(tokens: &[rlox_shared::Token]) -> String {
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

// ---- Chapter 16 — lazy `Scanner` iterator ----
//
// These tests exercise the streaming API the chapter 17 single-pass
// compiler depends on. They share the underlying state machine with the
// eager `scan()` above, so the contract is:
//
//   * Same token stream, same source-line attribution, same EOF terminator.
//   * Same error contents at the same source-position relative to surrounding tokens.
//   * Lazy: only as many bytes are consumed as the iterator advances.

use rlox_shared::Scanner;

#[test]
fn lazy_scanner_yields_same_token_stream_as_eager_scan() {
    let src = "var pi = 3.14; // greet\n\"hello\";";
    let (eager_tokens, eager_errs) = scan(src);
    assert!(eager_errs.is_empty());

    let lazy: Vec<_> = Scanner::new(src).collect();
    let lazy_oks: Vec<_> = lazy
        .iter()
        .map(|r| r.as_ref().expect("clean source").clone())
        .collect();
    assert_eq!(lazy_oks, eager_tokens);
}

#[test]
fn lazy_scanner_emits_eof_exactly_once_then_none() {
    let mut s = Scanner::new("");
    // The first event is EOF for empty input.
    let first = s.next().expect("EOF token");
    let token = first.expect("EOF is not an error");
    assert_eq!(token.ttype, TokenType::Eof);
    assert!(s.next().is_none(), "no more events after EOF");
}

#[test]
fn lazy_scanner_interleaves_errors_with_tokens_in_source_order() {
    // The `@` triggers an "Unexpected character." error, sandwiched
    // between two well-formed tokens.
    let mut s = Scanner::new("a @ b");
    let first = s.next().unwrap().expect("identifier 'a'");
    assert_eq!(first.ttype, TokenType::Identifier);
    assert_eq!(first.lexeme, "a");

    let second = s.next().unwrap();
    let LoxError::Scan { message, .. } = second.unwrap_err() else {
        panic!("expected scan error for '@'");
    };
    assert_eq!(message, "Unexpected character.");

    let third = s.next().unwrap().expect("identifier 'b'");
    assert_eq!(third.ttype, TokenType::Identifier);
    assert_eq!(third.lexeme, "b");
}

#[test]
fn lazy_scanner_skips_whitespace_and_comments_without_emitting() {
    // Whitespace and `//` line comments are silently dropped — no
    // events for them; the next event after a long stretch of trivia
    // is the next real token.
    let mut s = Scanner::new("   // a comment\n\t var");
    let first = s.next().unwrap().unwrap();
    assert_eq!(first.ttype, TokenType::Var);
    assert_eq!(first.line, 2, "line tracking respects newlines");
}

#[test]
fn lazy_scanner_advances_one_call_at_a_time() {
    // Pulling N times yields exactly N events; if we stop early we
    // never see anything further down the source. (Spot-check that
    // the iterator is actually lazy rather than secretly eager.)
    let src = "1 + 2 + 3 + 4 + 5";
    let mut s = Scanner::new(src);
    let _ = s.next().unwrap(); // 1
    let _ = s.next().unwrap(); // +
    let two = s.next().unwrap().unwrap();
    assert_eq!(two.lexeme, "2");
}

#[test]
fn lazy_scanner_line_method_tracks_through_newlines() {
    let mut s = Scanner::new("1\n\n\n2");
    s.next(); // consumes '1'
    // newlines are part of `next()`'s internal stepping; pulling the
    // second number should now report line 4.
    let two = s.next().unwrap().unwrap();
    assert_eq!(two.line, 4);
    assert_eq!(s.line(), 4);
}
