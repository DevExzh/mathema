use super::*;

#[test]
fn writes_a_complete_equation() {
    let result = crate::from_latex("x").unwrap();
    let bytes = result.to_mtef_bytes(&MtefWriteOptions::default()).unwrap();
    assert_eq!(bytes.last(), Some(&0));
    assert!(crate::from_mtef_bytes(&bytes).is_ok());
}
