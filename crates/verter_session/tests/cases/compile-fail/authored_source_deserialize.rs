use verter_type_expr::AuthoredTypeSource;

fn main() {
    let forged = r#"{"DeclBody":{"anchor":{"canonical_id":"/forged.ts","owner":{"kind":"Module","ordinal":0},"symbol":"Forged","space":"Type"},"path":[]}}"#;
    let _: AuthoredTypeSource = serde_json::from_str(forged).unwrap();
}
