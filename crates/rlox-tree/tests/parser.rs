//! Phase 4 — public-API tests for the recursive-descent parser.
//!
//! Reference: chapter 6 test `test/expressions/parse.lox`. Source / expected
//! output anchor the precedence and grouping behaviour of `rlox_tree::parse`.

use rlox_tree::{Expr, LoxError, Stmt, Value, parse, parse_program, scan};

/// Convenience: scan + parse a Lox expression source string.
fn parse_str(src: &str) -> Result<Expr, LoxError> {
    let (tokens, errors) = scan(src);
    assert!(errors.is_empty(), "scan errors: {errors:?}");
    parse(&tokens)
}

/// Convenience: scan + parse a Lox *program* (sequence of statements).
fn program_str(src: &str) -> Result<Vec<Stmt>, Vec<LoxError>> {
    let (tokens, errors) = scan(src);
    assert!(errors.is_empty(), "scan errors: {errors:?}");
    parse_program(&tokens)
}

#[test]
fn parses_number_literal() {
    let e = parse_str("123").unwrap();
    assert_eq!(e, Expr::Literal(Value::Number(123.0)));
}

#[test]
fn parses_string_literal_strips_quotes() {
    let e = parse_str(r#""hi""#).unwrap();
    assert_eq!(e, Expr::Literal(Value::String("hi".into())));
}

#[test]
fn parses_boolean_and_nil_keywords() {
    assert_eq!(parse_str("true").unwrap(), Expr::Literal(Value::Bool(true)));
    assert_eq!(
        parse_str("false").unwrap(),
        Expr::Literal(Value::Bool(false))
    );
    assert_eq!(parse_str("nil").unwrap(), Expr::Literal(Value::Nil));
}

#[test]
fn parses_grouping() {
    let e = parse_str("(1)").unwrap();
    assert_eq!(e.to_string(), "(group 1.0)");
}

#[test]
fn parses_unary_minus_and_bang() {
    assert_eq!(parse_str("-1").unwrap().to_string(), "(- 1.0)");
    assert_eq!(parse_str("!true").unwrap().to_string(), "(! true)");
    // Right-associative: `!!true` → `(! (! true))`.
    assert_eq!(parse_str("!!true").unwrap().to_string(), "(! (! true))");
}

#[test]
fn factor_binds_tighter_than_term() {
    let e = parse_str("1 + 2 * 3").unwrap();
    assert_eq!(e.to_string(), "(+ 1.0 (* 2.0 3.0))");
}

#[test]
fn term_is_left_associative() {
    let e = parse_str("1 + 2 + 3").unwrap();
    assert_eq!(e.to_string(), "(+ (+ 1.0 2.0) 3.0)");
}

#[test]
fn factor_is_left_associative() {
    let e = parse_str("8 / 4 / 2").unwrap();
    assert_eq!(e.to_string(), "(/ (/ 8.0 4.0) 2.0)");
}

#[test]
fn comparison_and_equality_chain() {
    // Equality is the loosest operator; comparisons bind tighter.
    let e = parse_str("1 < 2 == 3 > 4").unwrap();
    assert_eq!(e.to_string(), "(== (< 1.0 2.0) (> 3.0 4.0))");
}

#[test]
fn equality_is_left_associative() {
    let e = parse_str("1 == 2 == 3").unwrap();
    assert_eq!(e.to_string(), "(== (== 1.0 2.0) 3.0)");
}

#[test]
fn parses_chap06_reference_case() {
    // From upstream `test/expressions/parse.lox`:
    //     (5 - (3 - 1)) + -1
    //     // expect: (+ (group (- 5.0 (group (- 3.0 1.0)))) (- 1.0))
    let e = parse_str("(5 - (3 - 1)) + -1").unwrap();
    assert_eq!(
        e.to_string(),
        "(+ (group (- 5.0 (group (- 3.0 1.0)))) (- 1.0))"
    );
}

#[test]
fn missing_closing_paren_reports_parse_error() {
    let err = parse_str("(1 + 2").unwrap_err();
    let LoxError::Parse {
        line,
        location,
        message,
    } = err
    else {
        panic!("expected Parse error, got {err:?}");
    };
    assert_eq!(line, 1);
    assert_eq!(location, " at end");
    assert_eq!(message, "Expect ')' after expression.");
}

#[test]
fn missing_primary_reports_parse_error() {
    let err = parse_str("+ 1").unwrap_err();
    let LoxError::Parse {
        line,
        location,
        message,
    } = err
    else {
        panic!("expected Parse error, got {err:?}");
    };
    assert_eq!(line, 1);
    assert_eq!(location, " at '+'");
    assert_eq!(message, "Expect expression.");
}

#[test]
fn missing_operand_at_eof_reports_parse_error() {
    let err = parse_str("1 +").unwrap_err();
    let LoxError::Parse {
        line,
        location,
        message,
    } = err
    else {
        panic!("expected Parse error, got {err:?}");
    };
    assert_eq!(line, 1);
    assert_eq!(location, " at end");
    assert_eq!(message, "Expect expression.");
}

#[test]
fn rejects_trailing_garbage_after_complete_expression() {
    // `1 2` — `1` parses as a complete expression, the leftover `2` is
    // unexpected. The parser should surface this rather than silently
    // dropping tokens.
    let err = parse_str("1 2").unwrap_err();
    let LoxError::Parse {
        line,
        location,
        message,
    } = err
    else {
        panic!("expected Parse error, got {err:?}");
    };
    assert_eq!(line, 1);
    assert_eq!(location, " at '2'");
    assert_eq!(message, "Expect end of expression.");
}

// ---- chapter 8: programs (statements + variables) ----

#[test]
fn program_parses_print_statement() {
    let stmts = program_str("print 42;").unwrap();
    assert_eq!(stmts.len(), 1);
    assert_eq!(stmts[0].to_string(), "(print 42.0)");
}

#[test]
fn program_parses_expression_statement() {
    let stmts = program_str("1 + 2;").unwrap();
    assert_eq!(stmts.len(), 1);
    assert_eq!(stmts[0].to_string(), "(; (+ 1.0 2.0))");
}

#[test]
fn program_parses_var_declaration_with_and_without_initializer() {
    let stmts = program_str("var a = 1; var b;").unwrap();
    assert_eq!(stmts.len(), 2);
    assert_eq!(stmts[0].to_string(), "(var a 1.0)");
    assert_eq!(stmts[1].to_string(), "(var b)");
}

#[test]
fn program_parses_block_with_nested_scope() {
    let stmts = program_str("{ var a = 1; print a; }").unwrap();
    assert_eq!(stmts.len(), 1);
    assert_eq!(stmts[0].to_string(), "(block (var a 1.0) (print a))");
}

#[test]
fn program_parses_assignment_expression() {
    let stmts = program_str("a = 1;").unwrap();
    assert_eq!(stmts[0].to_string(), "(; (= a 1.0))");
}

#[test]
fn program_parses_assignment_as_right_associative() {
    // a = b = 1; ⇒ (= a (= b 1))
    let stmts = program_str("a = b = 1;").unwrap();
    assert_eq!(stmts[0].to_string(), "(; (= a (= b 1.0)))");
}

#[test]
fn program_rejects_invalid_assignment_target() {
    let errs = program_str("(a) = 1;").unwrap_err();
    let LoxError::Parse {
        location, message, ..
    } = &errs[0]
    else {
        panic!("expected Parse error, got {:?}", errs[0]);
    };
    assert_eq!(location, " at '='");
    assert_eq!(message, "Invalid assignment target.");
}

#[test]
fn program_requires_semicolon_after_expression_statement() {
    let errs = program_str("1 + 2").unwrap_err();
    assert!(
        errs.iter().any(|e| matches!(
            e,
            LoxError::Parse { message, .. } if message == "Expect ';' after expression."
        )),
        "expected `Expect ';' after expression.`, got {errs:?}"
    );
}

#[test]
fn program_requires_semicolon_after_print() {
    let errs = program_str("print 1").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect ';' after value."
    )));
}

#[test]
fn program_requires_close_brace_for_block() {
    let errs = program_str("{ var a = 1;").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect '}' after block."
    )));
}

#[test]
fn program_synchronize_collects_multiple_errors() {
    // The first `var` declaration fails (no name); the parser should
    // synchronize on the trailing `;` and then surface the second error.
    let errs = program_str("var ;\nvar ;").unwrap_err();
    assert!(
        errs.len() >= 2,
        "expected >=2 errors, got {} : {errs:?}",
        errs.len()
    );
    assert!(errs.iter().all(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect variable name."
    )));
}

#[test]
fn program_synchronize_recovers_at_keyword_boundary() {
    // No `;` between the failing fragment and the next declaration; the
    // parser synchronises by stopping at the `print` keyword.
    let errs = program_str("var = 1; print 1;").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect variable name."
    )));
}

// ---- chapter 9: control flow + logical operators ----

#[test]
fn parses_if_without_else() {
    let stmts = program_str("if (true) print 1;").unwrap();
    assert_eq!(stmts[0].to_string(), "(if true (print 1.0))");
}

#[test]
fn parses_if_else() {
    let stmts = program_str("if (true) print 1; else print 2;").unwrap();
    assert_eq!(stmts[0].to_string(), "(if true (print 1.0) (print 2.0))");
}

#[test]
fn dangling_else_binds_to_nearest_if() {
    // `if (a) if (b) 1; else 2;` ⇒ the `else` belongs to the inner `if`.
    let stmts = program_str("if (1) if (2) print 3; else print 4;").unwrap();
    assert_eq!(
        stmts[0].to_string(),
        "(if 1.0 (if 2.0 (print 3.0) (print 4.0)))"
    );
}

#[test]
fn if_requires_parentheses_around_condition() {
    let errs = program_str("if true print 1;").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect '(' after 'if'."
    )));
}

#[test]
fn parses_while_loop() {
    let stmts = program_str("while (true) print 1;").unwrap();
    assert_eq!(stmts[0].to_string(), "(while true (print 1.0))");
}

#[test]
fn while_requires_parentheses_around_condition() {
    let errs = program_str("while true print 1;").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect '(' after 'while'."
    )));
}

#[test]
fn parses_logical_and_lower_precedence_than_equality() {
    // `a == b and c == d` ⇒ (and (== a b) (== c d))
    let stmts = program_str("a == b and c == d;").unwrap();
    assert_eq!(stmts[0].to_string(), "(; (and (== a b) (== c d)))");
}

#[test]
fn or_has_lower_precedence_than_and() {
    // `a or b and c` ⇒ (or a (and b c))
    let stmts = program_str("a or b and c;").unwrap();
    assert_eq!(stmts[0].to_string(), "(; (or a (and b c)))");
}

#[test]
fn for_loop_desugars_to_block_with_while() {
    // for (var i = 0; i < 3; i = i + 1) print i;
    // ⇒ (block (var i 0.0) (while (< i 3.0) (block (print i) (; (= i (+ i 1.0))))))
    let stmts = program_str("for (var i = 0; i < 3; i = i + 1) print i;").unwrap();
    assert_eq!(
        stmts[0].to_string(),
        "(block (var i 0.0) \
         (while (< i 3.0) \
         (block (print i) (; (= i (+ i 1.0))))))"
    );
}

#[test]
fn for_loop_with_omitted_clauses_defaults_to_true_condition() {
    // for (;;) ; ⇒ (while true (; nil))
    // — "; ;" is two empty expression statements: omitted-init/cond/incr,
    //   so we use a no-op-ish expression statement as the body. Use `;`
    //   isn't valid Lox; smallest valid body is an empty block `{}`.
    let stmts = program_str("for (;;) {}").unwrap();
    assert_eq!(stmts[0].to_string(), "(while true (block))");
}

#[test]
fn for_loop_without_initializer_skips_outer_block() {
    // No init ⇒ no outer Block wrapper.
    let stmts = program_str("for (; i < 3; i = i + 1) print i;").unwrap();
    assert_eq!(
        stmts[0].to_string(),
        "(while (< i 3.0) (block (print i) (; (= i (+ i 1.0)))))"
    );
}

// ---- chapter 10: function declarations, calls, return ----

#[test]
fn parses_call_with_no_arguments() {
    let stmts = program_str("clock();").unwrap();
    assert_eq!(stmts[0].to_string(), "(; (call clock))");
}

#[test]
fn parses_call_with_arguments() {
    let stmts = program_str("add(1, 2, 3);").unwrap();
    assert_eq!(stmts[0].to_string(), "(; (call add 1.0 2.0 3.0))");
}

#[test]
fn parses_chained_calls_left_associatively() {
    // `f(1)(2)` ⇒ (call (call f 1) 2)
    let stmts = program_str("f(1)(2);").unwrap();
    assert_eq!(stmts[0].to_string(), "(; (call (call f 1.0) 2.0))");
}

#[test]
fn call_requires_closing_paren() {
    let errs = program_str("f(1, 2;").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect ')' after arguments."
    )));
}

#[test]
fn parses_function_declaration() {
    let stmts = program_str("fun add(a, b) { return a + b; }").unwrap();
    assert_eq!(stmts[0].to_string(), "(fun add (a b) (return (+ a b)))");
}

#[test]
fn parses_function_with_no_parameters() {
    let stmts = program_str("fun greet() { print \"hi\"; }").unwrap();
    assert_eq!(stmts[0].to_string(), "(fun greet () (print hi))");
}

#[test]
fn function_requires_name() {
    let errs = program_str("fun () {}").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect function name."
    )));
}

#[test]
fn function_requires_parens_after_name() {
    let errs = program_str("fun greet {}").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect '(' after function name."
    )));
}

#[test]
fn function_requires_brace_before_body() {
    let errs = program_str("fun greet() print 1;").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect '{' before function body."
    )));
}

#[test]
fn parses_bare_return() {
    let stmts = program_str("fun f() { return; }").unwrap();
    assert_eq!(stmts[0].to_string(), "(fun f () (return))");
}

#[test]
fn parses_return_with_value() {
    let stmts = program_str("fun f() { return 42; }").unwrap();
    assert_eq!(stmts[0].to_string(), "(fun f () (return 42.0))");
}

// ---- chapter 12: classes, methods, properties, this ----

#[test]
fn parses_empty_class_declaration() {
    let stmts = program_str("class Foo {}").unwrap();
    assert_eq!(stmts[0].to_string(), "(class Foo)");
}

#[test]
fn parses_class_with_methods() {
    let stmts =
        program_str("class Greeter { greet() { print \"hi\"; } shout(msg) { print msg; } }")
            .unwrap();
    assert_eq!(
        stmts[0].to_string(),
        "(class Greeter (method greet () (print hi)) (method shout (msg) (print msg)))"
    );
}

#[test]
fn class_requires_name() {
    let errs = program_str("class {}").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect class name."
    )));
}

#[test]
fn class_requires_open_brace() {
    let errs = program_str("class Foo").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect '{' before class body."
    )));
}

#[test]
fn parses_property_get() {
    let stmts = program_str("a.b;").unwrap();
    assert_eq!(stmts[0].to_string(), "(; (. a b))");
}

#[test]
fn parses_property_set() {
    let stmts = program_str("a.b = 1;").unwrap();
    assert_eq!(stmts[0].to_string(), "(; (.= a b 1.0))");
}

#[test]
fn parses_chained_property_access() {
    // `a.b.c` ⇒ (. (. a b) c)
    let stmts = program_str("a.b.c;").unwrap();
    assert_eq!(stmts[0].to_string(), "(; (. (. a b) c))");
}

#[test]
fn parses_property_call_then_property() {
    // `a.b().c` ⇒ (. (call (. a b)) c)
    let stmts = program_str("a.b().c;").unwrap();
    assert_eq!(stmts[0].to_string(), "(; (. (call (. a b)) c))");
}

#[test]
fn parses_this_keyword() {
    let stmts = program_str("class C { m() { return this; } }").unwrap();
    assert_eq!(
        stmts[0].to_string(),
        "(class C (method m () (return this)))"
    );
}

#[test]
fn dot_requires_property_name() {
    let errs = program_str("a.;").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect property name after '.'."
    )));
}

// ---- chapter 13: inheritance + super ----

#[test]
fn parses_class_with_superclass() {
    let stmts = program_str("class Sub < Sup {}").unwrap();
    assert_eq!(stmts[0].to_string(), "(class Sub < Sup)");
}

#[test]
fn class_less_requires_superclass_name() {
    let errs = program_str("class Sub < {}").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect superclass name."
    )));
}

#[test]
fn parses_super_method() {
    let stmts = program_str("class Sub < Sup { m() { super.greet(); } }").unwrap();
    assert_eq!(
        stmts[0].to_string(),
        "(class Sub < Sup (method m () (; (call (super greet)))))"
    );
}

#[test]
fn super_requires_dot() {
    let errs = program_str("class Sub < Sup { m() { super; } }").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect '.' after 'super'."
    )));
}

#[test]
fn super_dot_requires_method_name() {
    let errs = program_str("class Sub < Sup { m() { super.; } }").unwrap_err();
    assert!(errs.iter().any(|e| matches!(
        e,
        LoxError::Parse { message, .. } if message == "Expect superclass method name."
    )));
}
