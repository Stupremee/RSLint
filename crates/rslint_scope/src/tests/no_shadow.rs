rule_test! {
    no_shadow,
    filter: DatalogLint::is_no_shadow,
    // Should pass
    { "var a=3; function b(x) { a++; return x + a; }; setTimeout(function() { b(a); }, 0);" },
    { "(function() { var doSomething = function doSomething() {}; doSomething() }())" },
    { "var arguments;\nfunction bar() { }" },
    { "var a=3; var b = (x) => { a++; return x + a; }; setTimeout(() => { b(a); }, 0);" },
    { "class A {}" },
    { "class A { constructor() { var a; } }" },
    { "(function() { var A = class A {}; })()" },
    { "{ var a; } var a;" }, // this case reports `no-redeclare`, not shadowing.
    { "{ let a; } let a;" },
    { "{ let a; } var a;" },
    { "{ let a; } function a() {}" },
    { "{ const a = 0; } const a = 1;" },
    { "{ const a = 0; } var a;" },
    { "{ const a = 0; } function a() {}" },
    { "function foo() { let a; } let a;" },
    { "function foo() { let a; } var a;" },
    { "function foo() { let a; } function a() {}" },
    { "function foo() { var a; } let a;" },
    { "function foo() { var a; } var a;" },
    { "function foo() { var a; } function a() {}" },
    { "function foo(a) { } let a;" },
    { "{ let a; } let a;" },
    { "{ let a; } var a;" },
    { "{ const a = 0; } const a = 1;" },
    { "{ const a = 0; } var a;" },
    { "function foo() { let a; } let a;" },
    { "function foo() { let a; } var a;" },
    { "function foo() { var a; } let a;" },
    { "function foo() { var a; } var a;" },
    { "function foo(a) { } let a;" },
    { "function foo() { var Object = 0; }" },
    { "function foo() { var top = 0; }", browser: true },
    { "var Object = 0;" },
    { "var top = 0;", browser: true },

    // Should fail
    {
        "function a(x) { var b = function c() { var x = 'foo'; }; }",
        errors: [DatalogLint::no_shadow("x", 11..12, 43..44, false)],
    },
    {
        "var a = (x) => { var b = () => { var x = 'foo'; }; }",
        errors: [DatalogLint::no_shadow("x", 9..10, 37..38, false)],
    },
    // TODO: Options for configuring shadowing
    {
        "function foo(a) { } var a;",
        errors: [DatalogLint::no_shadow("a", 13..14, 24..25, false)],
    },
    // TODO: Options for configuring shadowing
    {
        "function foo(a) { } function a() {}",
        errors: [DatalogLint::no_shadow("a", 13..14, 29..30, false)],
    },
    {
        "var x = 1; { let x = 2; }",
        errors: [DatalogLint::no_shadow("x", 4..5, 17..18, false)],
    },
    {
        "let x = 1; { const x = 2; }",
        errors: [DatalogLint::no_shadow("x", 4..5, 19..20, false)],
    },
    {
        "{ let a; } function a() {}",
        errors: [DatalogLint::no_shadow("a", 4..5, 19..20, false)],
    },
    {
        "{ const a = 0; } function a() {}",
        errors: [DatalogLint::no_shadow("a", 4..5, 19..20, false)],
    },
    {
        "function foo() { let a; } function a() {}",
        errors: [DatalogLint::no_shadow("a", 4..5, 19..20, false)],
    },
    {
        "function foo() { var a; } function a() {}",
        errors: [DatalogLint::no_shadow("a", 4..5, 19..20, false)],
    },
}
