use nwscript_parser::{parse, Declaration};

#[test]
fn parse_basic_fixture() {
    let source = include_str!("../../../test-fixtures/basic.nss");
    let parsed = parse(source);

    // Should have: 1 include, 2 consts, 1 struct, 1 prototype, 2 function defs = 7 declarations
    assert_eq!(parsed.declarations.len(), 7, "expected 7 declarations");
    assert!(parsed.errors.is_empty(), "unexpected errors: {:?}", parsed.errors);

    // Verify types
    assert!(matches!(&parsed.declarations[0], Declaration::Include(_)));
    assert!(matches!(&parsed.declarations[1], Declaration::GlobalVar(v) if v.is_const));
    assert!(matches!(&parsed.declarations[2], Declaration::GlobalVar(v) if v.is_const));
    assert!(matches!(&parsed.declarations[3], Declaration::Struct(_)));
    assert!(matches!(&parsed.declarations[4], Declaration::Function(f) if f.is_prototype()));
    assert!(matches!(&parsed.declarations[5], Declaration::Function(f) if !f.is_prototype()));
    assert!(matches!(&parsed.declarations[6], Declaration::Function(f) if !f.is_prototype()));
}
