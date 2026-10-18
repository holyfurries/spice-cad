fn main() {
    let root = std::env::var("SMLIB_ROOT")
        .expect("Enter nix develop to build the native SMLib application");
    let output = std::env::var("OUT_DIR").expect("Cargo provides OUT_DIR");
    std::fs::write(format!("{output}/spice_smlib_types.h"), "#include <SmApiBrep.h>\n#include <SmVector3d.h>\n#include <cstddef>\nusing SmFailureArray = SmTArray<SmTessellationFailure>;\nstatic_assert(sizeof(SmVector3d) == 24);\nstatic_assert(offsetof(SmVector3d, x) == 0);\nstatic_assert(offsetof(SmVector3d, y) == 8);\nstatic_assert(offsetof(SmVector3d, z) == 16);\n").expect("Write generated ABI declarations");
    cxx_build::bridge("src/kernel.rs")
        .include(output)
        .include(format!("{root}/include"))
        .define("SM_GRAPHICS_CALLBACKS", "1")
        .define("NDEBUG", "1")
        .define("SM_USE_TBB", "1")
        .define("SM_API_EXPORTS", None)
        .std("c++17")
        .opt_level(3)
        .compile("spice_smlib_bridge");
    println!("cargo:rustc-link-search=native={root}/lib");
    println!("cargo:rustc-link-lib=static=sm_api");
    println!("cargo:rustc-link-lib=static=smlib");
    println!("cargo:rustc-link-lib=tbb");
    println!("cargo:rustc-link-lib=stdc++");
    println!("cargo:rerun-if-changed=src/kernel.rs");
    println!("cargo:rerun-if-env-changed=SMLIB_ROOT");
}
