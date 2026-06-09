use super::*;

fn fmt(source: &str) -> String {
    format(source, &FormatConfig::default())
}

fn fmt_with(source: &str, config: &FormatConfig) -> String {
    format(source, config)
}

// =============================================================================
// Basic declarations
// =============================================================================

#[test]
fn format_empty_file() {
    assert_eq!(fmt(""), "\n");
}

#[test]
fn format_includes_sorted() {
    let input = r#"#include "nwnx_sql"
#include "nwnx_player"
#include "_tdn_constants"
"#;
    let expected = r#"#include "_tdn_constants"
#include "nwnx_player"
#include "nwnx_sql"
"#;
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_includes_unsorted_when_disabled() {
    let mut config = FormatConfig::default();
    config.sort_includes = false;
    let input = r#"#include "nwnx_sql"
#include "nwnx_player"
"#;
    let expected = r#"#include "nwnx_sql"
#include "nwnx_player"
"#;
    assert_eq!(fmt_with(input, &config), expected);
}

#[test]
fn format_global_const() {
    let input = "const int MY_CONST=42;";
    let expected = "const int MY_CONST = 42;\n";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_struct() {
    let input = "struct MyStruct{int nValue;string sName;};";
    let expected = "\
struct MyStruct
{
    int nValue;
    string sName;
};
";
    assert_eq!(fmt(input), expected);
}

// =============================================================================
// Function formatting
// =============================================================================

#[test]
fn format_function_prototype() {
    let input = "void DoSomething(  object oPC,int nParam  =  0  );";
    let expected = "void DoSomething(object oPC, int nParam = 0);\n";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_function_definition() {
    let input = "void main(){int x=5;return;}";
    let expected = "\
void main()
{
    int x = 5;
    return;
}
";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_function_wraps_long_params() {
    let mut config = FormatConfig::default();
    config.max_line_width = 40;
    let input = "void MyFunction(int nFirstParam, string sSecondParam, float fThirdParam)\n{\n}";
    let result = fmt_with(input, &config);
    // Should wrap parameters
    assert!(result.contains("\n    int nFirstParam"));
    assert!(result.contains(",\n    string sSecondParam"));
}

// =============================================================================
// Control flow
// =============================================================================

#[test]
fn format_if_else() {
    let input = "void main(){if(x>10){DoA();}else{DoB();}}";
    let expected = "\
void main()
{
    if (x > 10)
    {
        DoA();
    }
    else
    {
        DoB();
    }
}
";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_if_else_if() {
    let input = "void main(){if(x>10){DoA();}else if(x>5){DoB();}else{DoC();}}";
    let expected = "\
void main()
{
    if (x > 10)
    {
        DoA();
    }
    else if (x > 5)
    {
        DoB();
    }
    else
    {
        DoC();
    }
}
";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_while() {
    let input = "void main(){while(x>0){x=x-1;}}";
    let expected = "\
void main()
{
    while (x > 0)
    {
        x = x - 1;
    }
}
";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_do_while() {
    let input = "void main(){do{x=x+1;}while(x<10);}";
    let expected = "\
void main()
{
    do
    {
        x = x + 1;
    }
    while (x < 10);
}
";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_for_loop() {
    let input = "void main(){int i;for(i=0;i<10;i++){DoThing(i);}}";
    let expected = "\
void main()
{
    int i;
    for (i = 0; i < 10; i++)
    {
        DoThing(i);
    }
}
";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_switch() {
    let input = "void main(){switch(x){case 1:DoA();break;case 2:DoB();break;default:DoC();break;}}";
    let expected = "\
void main()
{
    switch (x)
    {
        case 1:
            DoA();
            break;
        case 2:
            DoB();
            break;
        default:
            DoC();
            break;
    }
}
";
    assert_eq!(fmt(input), expected);
}

// =============================================================================
// Expressions
// =============================================================================

#[test]
fn format_binary_operators() {
    let input = "void main(){int x=a+b*c;}";
    let expected = "\
void main()
{
    int x = a + b * c;
}
";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_function_call() {
    let input = "void main(){SendMessageToPC(oPC,\"hello\");}";
    let expected = "\
void main()
{
    SendMessageToPC(oPC, \"hello\");
}
";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_assignment_operators() {
    let input = "void main(){x+=5;y-=3;z*=2;}";
    let expected = "\
void main()
{
    x += 5;
    y -= 3;
    z *= 2;
}
";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_ternary() {
    let input = "void main(){int x=a>b?1:0;}";
    let expected = "\
void main()
{
    int x = a > b ? 1 : 0;
}
";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_unary_and_postfix() {
    let input = "void main(){int x=-5;int y=!z;x++;--y;}";
    let expected = "\
void main()
{
    int x = -5;
    int y = !z;
    x++;
    --y;
}
";
    assert_eq!(fmt(input), expected);
}

// =============================================================================
// Comments
// =============================================================================

#[test]
fn format_preserves_line_comments() {
    let input = "\
// This is a header comment
void main()
{
    // Inside function
    int x = 5;
}
";
    let result = fmt(input);
    assert!(result.contains("// This is a header comment"));
    assert!(result.contains("// Inside function"));
}

#[test]
fn format_preserves_trailing_comments() {
    let input = "\
void main()
{
    int x = 5; // important value
}
";
    let result = fmt(input);
    assert!(result.contains("int x = 5;  // important value"));
}

// =============================================================================
// Blank line normalization
// =============================================================================

#[test]
fn format_blank_lines_between_functions() {
    let input = "\
void FuncA()
{
}



void FuncB()
{
}
";
    let result = fmt(input);
    // Should have exactly one blank line between functions
    assert!(result.contains("}\n\nvoid FuncB()"));
    // Should not have more than one blank line
    assert!(!result.contains("}\n\n\nvoid FuncB()"));
}

// =============================================================================
// Brace style configuration
// =============================================================================

#[test]
fn format_same_line_braces() {
    let mut config = FormatConfig::default();
    config.brace_style = BraceStyle::SameLine;
    let input = "void main()\n{\nint x=5;\n}";
    let result = fmt_with(input, &config);
    assert!(result.contains("void main() {"));
    assert!(result.contains("    int x = 5;"));
}

// =============================================================================
// Spacing configuration
// =============================================================================

#[test]
fn format_no_space_after_keywords() {
    let mut config = FormatConfig::default();
    config.space_after_keywords = false;
    let input = "void main(){if (x>0){return;}}";
    let result = fmt_with(input, &config);
    assert!(result.contains("if(x > 0)"));
}

#[test]
fn format_no_space_around_operators() {
    let mut config = FormatConfig::default();
    config.space_around_operators = false;
    let input = "void main(){int x = a + b;}";
    let result = fmt_with(input, &config);
    assert!(result.contains("int x=a+b;"));
}

// =============================================================================
// Edge cases
// =============================================================================

#[test]
fn format_braceless_if_gets_braces() {
    let input = "void main(){if(x>0) DoA();}";
    let result = fmt(input);
    // Should wrap single statement in braces
    assert!(result.contains("if (x > 0)\n    {\n        DoA();\n    }"));
}

#[test]
fn format_field_access() {
    let input = "void main(){sData.nValue=5;}";
    let expected = "\
void main()
{
    sData.nValue = 5;
}
";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_nested_calls() {
    let input = "void main(){SendMessageToPC(GetFirstPC(),IntToString(42));}";
    let expected = "\
void main()
{
    SendMessageToPC(GetFirstPC(), IntToString(42));
}
";
    assert_eq!(fmt(input), expected);
}

#[test]
fn format_preserves_blank_line_grouping() {
    let input = "\
void main()
{
    int x = 1;
    int y = 2;

    int z = 3;
}
";
    let result = fmt(input);
    // Should preserve the blank line between y and z
    assert!(result.contains("int y = 2;\n\n    int z = 3;"));
}

#[test]
fn format_idempotent() {
    let input = "\
#include \"nwnx_player\"

const int MY_CONST = 42;

void main()
{
    if (x > 0)
    {
        DoA();
    }
    else
    {
        DoB();
    }
}
";
    let first = fmt(input);
    let second = fmt(&first);
    assert_eq!(first, second, "Formatting should be idempotent");
}

#[test]
fn format_includes_with_header_comment() {
    let input = "\
// File header comment
#include \"b_file\"
#include \"a_file\"
";
    let result = fmt(input);
    assert!(result.starts_with("// File header comment\n"));
    // Includes should be sorted
    assert!(result.contains("#include \"a_file\"\n#include \"b_file\""));
}

#[test]
fn format_multiple_declarations_separated() {
    let input = "const int A=1;const int B=2;void Func(){return;}";
    let result = fmt(input);
    // Each declaration should have a blank line between them
    assert!(result.contains("const int A = 1;\n\nconst int B = 2;\n\nvoid Func()"));
}

// =============================================================================
// Full file formatting (integration)
// =============================================================================

#[test]
fn format_full_file() {
    let input = r#"// Basic NWScript test file for parser validation.
#include "nwnx_player"

const int MY_CONSTANT = 42;
const string MY_STRING = "hello";

struct MyStruct
{
    int nValue;
    string sName;
    float fWeight;
};

// Forward declaration
void DoSomething(object oPC, int nParam = 0);

// Function with body
void main()
{
    object oPC = GetFirstPC();
    int nLevel = GetHitDice(oPC);

    if (nLevel > 10)
    {
        SendMessageToPC(oPC, "High level!");
    }
    else
    {
        SendMessageToPC(oPC, "Keep going.");
    }

    struct MyStruct sData;
    sData.nValue = nLevel;
    sData.sName = GetName(oPC);

    int i;
    for (i = 0; i < 10; i++)
    {
        // Loop body
        int nTemp = i * 2;
    }

    switch (nLevel)
    {
        case 1:
            break;
        case 5:
            DoSomething(oPC, 1);
            break;
        default:
            DoSomething(oPC);
            break;
    }

    return;
}

void DoSomething(object oPC, int nParam = 0)
{
    string sMsg = "Param: " + IntToString(nParam);
    SendMessageToPC(oPC, sMsg);
}
"#;

    let result = fmt(input);
    // The formatter should produce clean, readable output
    assert!(result.contains("struct MyStruct\n{"));
    assert!(result.contains("void main()\n{"));
    assert!(result.contains("if (nLevel > 10)\n    {"));
    // It should sort includes (only one here, so order doesn't change)
    assert!(result.contains("#include \"nwnx_player\""));
    // Should end with newline
    assert!(result.ends_with('\n'));
}
