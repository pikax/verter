use std::env;

use cmake::Config;

// Verter vendors only the v3 source tree (`c_src/mimalloc3`).
#[cfg(not(feature = "v3"))]
compile_error!("this vendored libmimalloc-sys2 carries only the mimalloc v3 sources: enable feature `v3`");

fn main() {
    #[cfg(not(feature = "v3"))]
    let mut cmake_config = Config::new("c_src/mimalloc");
    #[cfg(feature = "v3")]
    let mut cmake_config = Config::new("c_src/mimalloc3");

    cmake_config
        .define("MI_BUILD_STATIC", "ON")
        .define("MI_BUILD_OBJECT", "OFF")
        .define("MI_BUILD_SHARED", "OFF")
        .define("MI_BUILD_TESTS", "OFF");

    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("target_os not defined!");
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").expect("target_arch not defined!");
    let target_env = env::var("CARGO_CFG_TARGET_ENV").expect("target_env not defined!");
    let profile = env::var("PROFILE").expect("profile not defined!");

    if target_os == "windows" && target_env == "msvc" {
        build_mimalloc_win();
        return;
    }

    if env::var_os("CARGO_FEATURE_OVERRIDE").is_some() {
        cmake_config.define("MI_OVERRIDE", "ON");
    } else {
        cmake_config.define("MI_OVERRIDE", "OFF");
    }

    if env::var_os("CARGO_FEATURE_ASM").is_some() {
        cmake_config.define("MI_SEE_ASM", "ON");
    }

    if env::var_os("CARGO_FEATURE_SKIP_COLLECT_ON_EXIT").is_some() {
        cmake_config.define("MI_SKIP_COLLECT_ON_EXIT", "ON");
    }

    let secure = env::var_os("CARGO_FEATURE_SECURE").is_some();
    if secure {
        cmake_config.define("MI_SECURE", "ON");
    }

    if env::var_os("CARGO_FEATURE_ETW").is_some() {
        cmake_config.define("MI_TRACK_ETW", "ON");
    }

    if env::var_os("CARGO_FEATURE_NO_OPT_ARCH").is_some() {
        cmake_config
            .define("MI_OPT_ARCH", "OFF")
            .define("MI_NO_OPT_ARCH", "ON");
    }

    if profile == "debug" {
        cmake_config
            .define("MI_DEBUG_FULL", "ON")
            .define("MI_SHOW_ERRORS", "ON");
    }

    if target_env == "musl" {
        cmake_config
            .define("MI_LIBC_MUSL", "ON")
            .cflag("-Wno-error=date-time");
        if target_arch == "aarch64" {
            cmake_config
                .define("MI_OPT_ARCH", "OFF")
                .define("MI_NO_OPT_ARCH", "ON");
        }
    }

    let dynamic_tls = env::var("CARGO_FEATURE_LOCAL_DYNAMIC_TLS").is_ok();

    if dynamic_tls {
        cmake_config.define("MI_LOCAL_DYNAMIC_TLS", "ON");
    }

    // On macOS, mimalloc v3 defaults to a fixed TLS slot (TCB[108]/[109]) shared by
    // every image, so a second statically-linked copy (e.g. another napi addon)
    // adopts the first's per-thread heap and crashes. Force the thread-local model
    // so each image gets its own `__thread` storage (a v3-only cmake option from
    // microsoft/mimalloc b83dee64, issue #1301).
    if env::var_os("CARGO_FEATURE_V3").is_some() && target_os == "macos" {
        cmake_config
            .define("MI_TLS_MODEL_LOCAL", "ON")
            // macOS may allocate on a thread-local's first access; guard against
            // re-entering malloc until the process is initialized.
            .define("MI_TLS_RECURSE_GUARD", "ON");
    }

    if (target_os == "linux" || target_os == "android")
        && env::var_os("CARGO_FEATURE_NO_THP").is_some()
    {
        cmake_config.define("MI_NO_THP", "1");
    }

    if env::var_os("CARGO_FEATURE_DEBUG").is_some()
        || (env::var_os("CARGO_FEATURE_DEBUG_IN_DEBUG").is_some() && cfg!(debug_assertions))
    {
        cmake_config.define("MI_DEBUG_FULL", "ON");
        cmake_config.define("MI_SHOW_ERRORS", "ON");
    } else {
        // Remove heavy debug assertions etc
        cmake_config.define("MI_DEBUG_FULL", "OFF");
    }

    // replicate naming logic from https://github.com/microsoft/mimalloc/blob/0ddf397796fbefa35b3278bd4431c2913a9892eb/CMakeLists.txt#L595-L599
    let profile_suffix = match cmake_config.get_profile() {
        p if p.eq_ignore_ascii_case("release")
            || p.eq_ignore_ascii_case("relwithdebinfo")
            || p.eq_ignore_ascii_case("minsizerel")
            || p.eq_ignore_ascii_case("none") =>
        {
            None
        }
        p => Some(format!("-{}", p.to_ascii_lowercase())),
    };

    let dst = cmake_config.build();

    println!("cargo:rustc-link-search=native={}/build", dst.display());

    println!(
        "cargo:rustc-link-lib=static=mimalloc{}{}",
        if secure { "-secure" } else { "" },
        profile_suffix.as_deref().unwrap_or("")
    );

    // on armv6 we need to link with libatomic
    if target_os == "linux" && target_arch == "arm" {
        // Embrace the atomic capability library across various platforms.
        // For instance, on certain platforms, llvm has relocated the atomic of the arm32 architecture to libclang_rt.builtins.a
        // while some use libatomic.a, and others use libatomic_ops.a.
        let atomic_name = env::var("DEP_ATOMIC").unwrap_or("atomic".to_owned());
        println!("cargo:rustc-link-lib={atomic_name}");
    }
}

fn build_mimalloc_win() {
    use std::env;

    let features = env::var("CARGO_CFG_TARGET_FEATURE")
        .map(|features| {
            features
                .split(",")
                .map(|f| f.to_owned())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    let mut build = cc::Build::new();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").expect("target_arch not defined!");

    // Route to the v3 source tree when the `v3` feature is enabled,
    // mirroring the cmake branch above. Both trees expose `src/static.c`
    // as a single translation unit that pulls in all the platform .c
    // files, so the cc-rs single-file build pattern works for either.
    let source_dir = if env::var_os("CARGO_FEATURE_V3").is_some() {
        "./c_src/mimalloc3"
    } else {
        "./c_src/mimalloc"
    };

    build
        .include(format!("{source_dir}/include"))
        .include(format!("{source_dir}/src"))
        .file(format!("{source_dir}/src/static.c"))
        .define("MI_BUILD_SHARED", "0")
        .cpp(true)
        .warnings(false)
        .flag_if_supported("-w");

    if env::var_os("CARGO_FEATURE_SECURE").is_some() {
        build.define("MI_SECURE", "4");
    }

    if env::var_os("CARGO_FEATURE_ASM").is_some() {
        build.flag_if_supported("-save-temps");
    }

    if env::var_os("CARGO_FEATURE_NO_OPT_ARCH").is_none() && target_arch == "arm64" {
        build.flag_if_supported("/arch:armv8.1");
    }

    if env::var_os("CARGO_FEATURE_SKIP_COLLECT_ON_EXIT").is_some() {
        build.define("MI_SKIP_COLLECT_ON_EXIT", "1");
    }

    // Verter: the release configuration in every Cargo profile. A dev or test
    // build then runs the allocator a release build ships instead of one with
    // mimalloc's assertion checking (`MI_DEBUG=2`, the default when `NDEBUG`
    // is unset) on every allocation. The cmake path above gets the same from
    // its optimised CMake build type, which defines `MI_DEBUG=0`.
    build.define("MI_DEBUG_FULL", "0").define("MI_DEBUG", "0");

    // Force the .c source to be compiled as C++ (equivalent to mimalloc upstream's
    // `MI_USE_CXX=ON`). The C-mode `_MSC_VER` branch in `atomic.h` references
    // MSVC-only ARM64 intrinsics (`__ldar64`/`__stlr64`) that clang does not
    // declare, so `aarch64-pc-windows-msvc` builds fail under `TARGET_CC=clang`.
    // Compiling as C++ takes the `<atomic>` path and avoids that branch entirely.
    // `cpp(true)` alone is insufficient because cc-rs does not inject `/TP` or
    // `-x c++`; the language must be forced explicitly.
    if build.get_compiler().is_like_msvc() {
        build.flag("/TP");
    } else {
        build.flag("-xc++");
    }

    const LIBS: [&str; 5] = ["psapi", "shell32", "user32", "advapi32", "bcrypt"];

    for lib in LIBS {
        println!("cargo:rustc-link-lib={lib}");
    }

    if features.contains(&"crt-static".to_string()) {
        build.static_crt(true);
    }

    build.compile("mimalloc_safe_static");
}
