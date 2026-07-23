//! Small assertion macros shared by real-provider tests.

/// Assert that a documented provider limitation still exists. Once the
/// condition becomes false the canary fails, forcing promotion to a normal
/// positive assertion instead of silently retaining obsolete coverage.
macro_rules! canary_assert_known_limitation {
    ($broken_cond:expr, $($arg:tt)+) => {
        if $broken_cond {
            eprintln!(
                "  CANARY (known limitation still present): {}",
                format_args!($($arg)+)
            );
        } else {
            panic!(
                "CANARY RESOLVED — limitation no longer present, promote to real assert!: {}",
                format_args!($($arg)+)
            );
        }
    };
}

pub(crate) use canary_assert_known_limitation;
