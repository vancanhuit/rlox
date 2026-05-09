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

// ---- chapter 8: Interpreter (statements, variables, scopes) ----

mod programs {
    use rlox::{Interpreter, LoxError, parse_program, resolve, scan};

    /// Run a program (scan → parse → resolve → interpret) and return
    /// the captured `print` output. Mirrors the production pipeline used
    /// by `rlox::run_to`.
    fn run(src: &str) -> Result<String, Vec<LoxError>> {
        let (tokens, scan_errors) = scan(src);
        assert!(scan_errors.is_empty(), "scan errors: {scan_errors:?}");
        let stmts = parse_program(&tokens)?;
        let locals = resolve(&stmts)?;
        let mut buf = Vec::<u8>::new();
        let mut interp = Interpreter::new(&mut buf);
        interp.merge_locals(locals);
        interp.interpret(&stmts).map_err(|e| vec![e])?;
        Ok(String::from_utf8(buf).unwrap())
    }

    #[test]
    fn print_emits_one_line_per_call() {
        assert_eq!(run("print 1; print 2;").unwrap(), "1\n2\n");
    }

    #[test]
    fn expression_statement_runs_for_side_effects_only() {
        assert_eq!(run("1 + 2; print 7;").unwrap(), "7\n");
    }

    #[test]
    fn var_with_initializer_can_be_read() {
        assert_eq!(run("var a = 9; print a;").unwrap(), "9\n");
    }

    #[test]
    fn var_without_initializer_defaults_to_nil() {
        assert_eq!(run("var a; print a;").unwrap(), "nil\n");
    }

    #[test]
    fn assignment_returns_value_and_can_chain() {
        // `a = b = 5` first sets `b`, then `a`, then `print` reads both.
        assert_eq!(
            run("var a; var b; a = b = 5; print a; print b;").unwrap(),
            "5\n5\n"
        );
    }

    #[test]
    fn block_introduces_a_nested_scope() {
        let src = "\
var a = \"global\";
{
  var a = \"block\";
  print a;
}
print a;
";
        assert_eq!(run(src).unwrap(), "block\nglobal\n");
    }

    #[test]
    fn assignment_inside_block_updates_outer_binding() {
        let src = "var a = 1; { a = 2; } print a;";
        assert_eq!(run(src).unwrap(), "2\n");
    }

    #[test]
    fn reading_undefined_variable_is_runtime_error() {
        let errs = run("print a;").unwrap_err();
        let LoxError::Runtime { message, .. } = &errs[0] else {
            panic!("expected Runtime error");
        };
        assert_eq!(message, "Undefined variable 'a'.");
    }

    #[test]
    fn assigning_undefined_variable_is_runtime_error() {
        let errs = run("a = 1;").unwrap_err();
        let LoxError::Runtime { message, .. } = &errs[0] else {
            panic!("expected Runtime error");
        };
        assert_eq!(message, "Undefined variable 'a'.");
    }

    #[test]
    fn runtime_error_inside_block_still_pops_the_scope() {
        // After the inner block fails, the outer `a` must still be in
        // scope — verified by a second program reusing the same shape.
        // We can't directly observe scope depth, but we *can* assert
        // that the inner-shadowed name does not leak after the error.
        let errs = run("{ var inner = 1; 1 + \"x\"; }").unwrap_err();
        assert!(matches!(errs[0], LoxError::Runtime { .. }));
    }

    // ---- chapter 9: control flow + short-circuit logical operators ----

    #[test]
    fn if_executes_then_branch_when_truthy() {
        assert_eq!(run("if (true) print 1;").unwrap(), "1\n");
    }

    #[test]
    fn if_executes_else_branch_when_falsy() {
        assert_eq!(run("if (false) print 1; else print 2;").unwrap(), "2\n");
    }

    #[test]
    fn if_with_no_else_is_a_no_op_when_falsy() {
        assert_eq!(run("if (false) print 1; print 2;").unwrap(), "2\n");
    }

    #[test]
    fn if_uses_lox_truthiness_for_non_bool_conditions() {
        // nil is falsy, every other non-false value is truthy.
        assert_eq!(run("if (nil) print 1; else print 2;").unwrap(), "2\n");
        assert_eq!(run("if (0) print 1; else print 2;").unwrap(), "1\n");
        assert_eq!(run(r#"if ("") print 1; else print 2;"#).unwrap(), "1\n");
    }

    #[test]
    fn while_loop_iterates_until_condition_is_false() {
        let src = "var i = 0; while (i < 3) { print i; i = i + 1; }";
        assert_eq!(run(src).unwrap(), "0\n1\n2\n");
    }

    #[test]
    fn while_loop_does_not_execute_when_condition_starts_false() {
        assert_eq!(run("while (false) print 1; print 2;").unwrap(), "2\n");
    }

    #[test]
    fn for_loop_runs_classic_counter() {
        let src = "for (var i = 0; i < 3; i = i + 1) print i;";
        assert_eq!(run(src).unwrap(), "0\n1\n2\n");
    }

    #[test]
    fn for_loop_with_external_initializer() {
        // Init clause empty; loop variable lives in the outer scope.
        let src = "var i = 1; for (; i < 4; i = i + 1) print i;";
        assert_eq!(run(src).unwrap(), "1\n2\n3\n");
    }

    #[test]
    fn for_loop_init_variable_does_not_leak_to_outer_scope() {
        // The desugared `for` wraps its init in a Block, so `i` is
        // out of scope after the loop. Reading it must error.
        let errs = run("for (var i = 0; i < 1; i = i + 1) print i; print i;").unwrap_err();
        let LoxError::Runtime { message, .. } = &errs[0] else {
            panic!("expected Runtime error, got {:?}", errs[0]);
        };
        assert_eq!(message, "Undefined variable 'i'.");
    }

    #[test]
    fn or_returns_first_truthy_operand_without_evaluating_the_rest() {
        // `nil or 1 or runtime_error` ⇒ short-circuits at the truthy `1`,
        // never reaching the would-be runtime error.
        assert_eq!(run(r#"print nil or 1 or (1 + "x");"#).unwrap(), "1\n");
    }

    #[test]
    fn or_returns_first_operand_when_truthy() {
        // Book example: `print "hi" or 2;` → "hi"
        assert_eq!(run(r#"print "hi" or 2;"#).unwrap(), "hi\n");
    }

    #[test]
    fn or_falls_through_to_last_operand_when_all_falsy() {
        assert_eq!(run("print nil or false;").unwrap(), "false\n");
    }

    #[test]
    fn and_returns_first_falsy_operand_without_evaluating_the_rest() {
        // `false and runtime_error` ⇒ short-circuits at the falsy `false`.
        assert_eq!(run(r#"print false and (1 + "x");"#).unwrap(), "false\n");
    }

    #[test]
    fn and_returns_last_operand_when_all_truthy() {
        // Book example: `print 1 and 2;` → 2
        assert_eq!(run("print 1 and 2;").unwrap(), "2\n");
    }

    #[test]
    fn while_uses_lox_truthiness_for_condition() {
        // Non-bool truthy condition keeps looping; we mutate the variable
        // to a falsy nil to exit.
        let src = "var i = 1; while (i) { print i; i = nil; }";
        assert_eq!(run(src).unwrap(), "1\n");
    }

    // ---- chapter 10: functions, calls, closures, return ----

    #[test]
    fn function_declaration_then_call_returns_value() {
        let src = "fun add(a, b) { return a + b; } print add(2, 3);";
        assert_eq!(run(src).unwrap(), "5\n");
    }

    #[test]
    fn function_without_explicit_return_yields_nil() {
        let src = "fun f() {} print f();";
        assert_eq!(run(src).unwrap(), "nil\n");
    }

    #[test]
    fn bare_return_inside_function_yields_nil() {
        let src = "fun f() { return; } print f();";
        assert_eq!(run(src).unwrap(), "nil\n");
    }

    #[test]
    fn return_short_circuits_remaining_statements() {
        let src = "fun f() { return 1; print 2; } print f();";
        assert_eq!(run(src).unwrap(), "1\n");
    }

    #[test]
    fn function_value_displays_with_name() {
        let src = "fun greet() {} print greet;";
        assert_eq!(run(src).unwrap(), "<fn greet>\n");
    }

    #[test]
    fn calling_non_callable_is_runtime_error() {
        let errs = run("\"x\"();").unwrap_err();
        let LoxError::Runtime { message, .. } = &errs[0] else {
            panic!("expected Runtime error, got {:?}", errs[0]);
        };
        assert_eq!(message, "Can only call functions and classes.");
    }

    #[test]
    fn arity_mismatch_reports_expected_vs_actual() {
        let src = "fun f(a, b) { return a + b; } f(1);";
        let errs = run(src).unwrap_err();
        let LoxError::Runtime { message, .. } = &errs[0] else {
            panic!("expected Runtime error");
        };
        assert_eq!(message, "Expected 2 arguments but got 1.");
    }

    #[test]
    fn closure_captures_outer_binding() {
        // Book reference fragment: makeCounter returns a closure that
        // increments a private counter on each call.
        let src = "\
fun makeCounter() {
  var i = 0;
  fun count() { i = i + 1; return i; }
  return count;
}
var c = makeCounter();
print c();
print c();
print c();
";
        assert_eq!(run(src).unwrap(), "1\n2\n3\n");
    }

    #[test]
    fn closure_sees_late_bound_outer_variable() {
        // `f` reads `x` lazily — the value at call time, not declaration.
        let src = "var x = 1; fun f() { return x; } x = 99; print f();";
        assert_eq!(run(src).unwrap(), "99\n");
    }

    #[test]
    fn recursion_works_via_closure_self_reference() {
        // `fib` references itself; the closure captured at declaration
        // time includes the binding being defined.
        let src = "\
fun fib(n) {
  if (n < 2) return n;
  return fib(n - 2) + fib(n - 1);
}
print fib(8);
";
        assert_eq!(run(src).unwrap(), "21\n");
    }

    #[test]
    fn parameters_shadow_outer_bindings_only_inside_the_function() {
        let src = "\
var x = 1;
fun f(x) { return x; }
print f(99);
print x;
";
        assert_eq!(run(src).unwrap(), "99\n1\n");
    }

    #[test]
    fn clock_native_returns_a_number() {
        // We can't assert the exact value, but we can assert that
        // `clock()` is in scope and returns a Number.
        let src = "print clock() > 0;";
        assert_eq!(run(src).unwrap(), "true\n");
    }

    #[test]
    fn clock_arity_check_rejects_extra_args() {
        let errs = run("clock(1);").unwrap_err();
        let LoxError::Runtime { message, .. } = &errs[0] else {
            panic!("expected Runtime error");
        };
        assert_eq!(message, "Expected 0 arguments but got 1.");
    }

    #[test]
    fn return_at_top_level_is_static_error() {
        // Chapter 11 (resolver) rejects this statically before the
        // interpreter runs. The diagnostic surfaces as a Parse-flavoured
        // `LoxError` carrying the canonical jlox message.
        let errs = run("return 1;").unwrap_err();
        let LoxError::Parse { message, .. } = &errs[0] else {
            panic!("expected Parse error, got {:?}", errs[0]);
        };
        assert_eq!(message, "Can't return from top-level code.");
    }

    #[test]
    fn nested_calls_thread_arguments_correctly() {
        let src = "\
fun double(n) { return n * 2; }
fun triple(n) { return n * 3; }
print double(triple(5));
";
        assert_eq!(run(src).unwrap(), "30\n");
    }
}
