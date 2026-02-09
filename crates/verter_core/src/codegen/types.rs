pub struct ImportInfo {
    pub source: String,
    pub type_only: bool,
    pub specifiers: Vec<ImportInfoSpecifier>,
}

pub struct ImportInfoSpecifier {
    pub name: String,
    pub alias: Option<String>,
    pub is_type: bool,
}
