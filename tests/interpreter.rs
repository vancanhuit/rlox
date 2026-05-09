//! Phase 5 — public-API tests for the tree-walk interpreter.
//!
//! Reference: chapter 7 test `test/expressions/evaluate.lox`. The runtime
//! semantics (truthiness, equality, arithmetic, string concatenation, error
//! messages) follow the book.

use rlox::{Expr, LoxError, Value, evaluate, parse, scan, stringify};

fn eval_str(src: &str) -> Result<Value, LoxError> {
    let (tokens, scan_errors) = scan(src);
    assert!(scan_errors.is_empty(), "scan errors: {scan_errors:?}");
    let expr: Expr = parse(&tokens).expect("parse should succeed in eval_str");
    evaluate(&expr)
}

// ---- literals & grouping ----

#[test]
fn evaluates_number_string_bool_nil_literals() {
    assert_eq!(eval_str("123").unwrap(), Value::Number(123.0));
    assert_eq!(eval_str(r#""hi""#).unwrap(), Value::String("hi".into()));
    assert_eq!(eval_str("true").unwrap(), Value::Bool(true));
    assert_eq!(eval_str("false").unwrap(), Value::Bool(false));
    assert_eq!(eval_str("nil").unwrap(), Value::Nil);
}

#[test]
fn evaluates_grouping() {
    assert_eq!(eval_str("(1 + 2) * 3").unwrap(), Value::Number(9.0));
}

// ---- unary ----

#[test]
fn unary_minus_negates_number() {
    assert_eq!(eval_str("-1").unwrap(), Value::Number(-1.0));
    assert_eq!(eval_str("--1").unwrap(), Value::Number(1.0));
}

#[test]
fn unary_minus_on_non_number_is_runtime_error() {
    let err = eval_str(r#"-"hi""#).unwrap_err();
    let LoxError::Runtime { line, message } = err else {
        panic!("expected Runtime, got {err:?}");
    };
    assert_eq!(line, 1);
    assert_eq!(message, "Operand must be a number.");
}

#[test]
fn bang_uses_lox_truthiness() {
    // Only `nil` and `false` are falsy in Lox.
    assert_eq!(eval_str("!true").unwrap(), Value::Bool(false));
    assert_eq!(eval_str("!false").unwrap(), Value::Bool(true));
    assert_eq!(eval_str("!nil").unwrap(), Value::Bool(true));
    // Numbers (incl. 0) and strings (incl. empty) are truthy.
    assert_eq!(eval_str("!0").unwrap(), Value::Bool(false));
    assert_eq!(eval_str(r#"!"""#).unwrap(), Value::Bool(false));
    assert_eq!(eval_str(r#"!"hi""#).unwrap(), Value::Bool(false));
}

// ---- binary arithmetic ----

#[test]
fn arithmetic_on_numbers() {
    assert_eq!(eval_str("1 + 2").unwrap(), Value::Number(3.0));
    assert_eq!(eval_str("5 - 2").unwrap(), Value::Number(3.0));
    assert_eq!(eval_str("3 * 4").unwrap(), Value::Number(12.0));
    assert_eq!(eval_str("8 / 2").unwrap(), Value::Number(4.0));
}

#[test]
fn plus_concatenates_two_strings() {
    assert_eq!(
        eval_str(r#""foo" + "bar""#).unwrap(),
        Value::String("foobar".into()),
    );
}

#[test]
fn plus_with_mixed_operand_types_is_runtime_error() {
    let err = eval_str(r#"1 + "x""#).unwrap_err();
    let LoxError::Runtime { line, message } = err else {
        panic!("expected Runtime, got {err:?}");
    };
    assert_eq!(line, 1);
    assert_eq!(message, "Operands must be two numbers or two strings.");
}

#[test]
fn arithmetic_on_non_number_is_runtime_error() {
    for src in [r#""a" - 1"#, r#"1 * "b""#, r#""a" / 2"#] {
        let err = eval_str(src).unwrap_err();
        let LoxError::Runtime { line, message } = err else {
            panic!("expected Runtime for {src:?}, got {err:?}");
        };
        assert_eq!(line, 1);
        assert_eq!(message, "Operands must be numbers.");
    }
}

// ---- comparison & equality ----

#[test]
fn comparison_on_numbers() {
    assert_eq!(eval_str("1 < 2").unwrap(), Value::Bool(true));
    assert_eq!(eval_str("2 < 2").unwrap(), Value::Bool(false));
    assert_eq!(eval_str("2 <= 2").unwrap(), Value::Bool(true));
    assert_eq!(eval_str("3 > 2").unwrap(), Value::Bool(true));
    assert_eq!(eval_str("2 >= 3").unwrap(), Value::Bool(false));
}

#[test]
fn comparison_on_non_number_is_runtime_error() {
    let err = eval_str(r#""a" < "b""#).unwrap_err();
    let LoxError::Runtime { message, .. } = err else {
        panic!("expected Runtime");
    };
    assert_eq!(message, "Operands must be numbers.");
}

#[test]
fn equality_compares_across_types() {
    assert_eq!(eval_str("nil == nil").unwrap(), Value::Bool(true));
    assert_eq!(eval_str("nil == false").unwrap(), Value::Bool(false));
    assert_eq!(eval_str("1 == 1").unwrap(), Value::Bool(true));
    assert_eq!(eval_str("1 == 2").unwrap(), Value::Bool(false));
    assert_eq!(eval_str(r#"1 == "1""#).unwrap(), Value::Bool(false));
    assert_eq!(eval_str(r#""a" == "a""#).unwrap(), Value::Bool(true));
    assert_eq!(eval_str("true != false").unwrap(), Value::Bool(true));
}

// ---- chap07 reference ----

#[test]
fn evaluates_chap07_reference_case() {
    // From upstream `test/expressions/evaluate.lox`:
    //     (5 - (3 - 1)) + -1
    //     // expect: 2
    let value = eval_str("(5 - (3 - 1)) + -1").unwrap();
    assert_eq!(value, Value::Number(2.0));
    assert_eq!(stringify(&value), "2");
}

// ---- stringify ----

#[test]
fn stringify_strips_trailing_zero_on_whole_numbers() {
    assert_eq!(stringify(&Value::Number(1.0)), "1");
    assert_eq!(stringify(&Value::Number(0.0)), "0");
    assert_eq!(stringify(&Value::Number(-3.0)), "-3");
}

#[test]
fn stringify_keeps_fractional_part() {
    assert_eq!(stringify(&Value::Number(1.5)), "1.5");
    assert_eq!(stringify(&Value::Number(2.5)), "2.5");
    assert_eq!(stringify(&Value::Number(-0.25)), "-0.25");
}

#[test]
fn stringify_other_atoms() {
    assert_eq!(stringify(&Value::Nil), "nil");
    assert_eq!(stringify(&Value::Bool(true)), "true");
    assert_eq!(stringify(&Value::Bool(false)), "false");
    assert_eq!(stringify(&Value::String("hi".into())), "hi");
}
