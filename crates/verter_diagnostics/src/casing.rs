//! Shared casing utility functions for diagnostic rules.

/// Returns true if a string contains any ASCII uppercase letter.
pub fn has_uppercase(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_uppercase())
}

/// Converts a camelCase or PascalCase string to kebab-case.
pub fn to_kebab_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                result.push('-');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

/// Converts a kebab-case string to PascalCase (`my-component` → `MyComponent`).
pub fn kebab_to_pascal_case(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut upper_next = true;
    for ch in input.chars() {
        if ch == '-' || ch == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            for up in ch.to_uppercase() {
                out.push(up);
            }
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Check if a string is PascalCase (starts with uppercase, no hyphens).
pub fn is_pascal_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.chars().next().unwrap();
    first.is_ascii_uppercase() && !s.contains('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_uppercase_cases() {
        assert!(has_uppercase("myProp"));
        assert!(has_uppercase("MyProp"));
        assert!(!has_uppercase("my-prop"));
        assert!(!has_uppercase("myprop"));
    }

    #[test]
    fn to_kebab_case_cases() {
        assert_eq!(to_kebab_case("headerContent"), "header-content");
        assert_eq!(to_kebab_case("HeaderContent"), "header-content");
        assert_eq!(to_kebab_case("header"), "header");
        assert_eq!(to_kebab_case("myProp"), "my-prop");
    }

    #[test]
    fn is_pascal_case_cases() {
        assert!(is_pascal_case("MyComponent"));
        assert!(!is_pascal_case("my-component"));
        assert!(!is_pascal_case("myComponent"));
        assert!(!is_pascal_case(""));
    }
}
